"""Portable tests for the analysis contract, plus optional real Chromium fixture smoke."""

from pathlib import Path
from subprocess import DEVNULL, Popen, TimeoutExpired
from tempfile import TemporaryDirectory
from threading import Event, Thread
import os
import signal
import unittest

from analyze import compare, median_interval, power_window, reduce_campaign
from collect import FIXTURE, FRAMES, FixtureServer, Handler, ROOT, check_ready, write_json


class AnalysisTests(unittest.TestCase):
    def test_balanced_sign_interval_and_margin(self):
        margin = (20, 0.05, False)
        self.assertEqual(median_interval(list(range(12))), [2, 9])
        self.assertEqual(compare([(75, 100)] * 12, margin)["status"], "verified win")
        self.assertEqual(compare([(125, 100)] * 12, margin)["status"], "loss")
        self.assertEqual(compare([(110, 100)] * 12, margin)["status"], "tie")
        self.assertEqual(compare([(75, 100)] * 9, margin)["status"], "inconclusive")
        self.assertEqual(compare([(75, 100)] * 12, margin, balanced=False)["status"], "inconclusive")
        self.assertEqual(compare([], margin)["status"], "native validation pending")

    def test_meter_clips_and_rejects_an_unbracketed_or_corrupted_window(self):
        start = {"qpc_ns": 0, "utc_ns": 500_000_000}
        end = {"qpc_ns": 1_000_000_000, "utc_ns": 1_500_000_000}
        trace = [(0, 0), (1_000_000_000, 2), (2_000_000_000, 4)]
        self.assertEqual(power_window(trace, start, end), dict(whole_pc_watts=2, whole_pc_joules=2))
        self.assertEqual(power_window(trace[1:], start, end), {})
        self.assertEqual(power_window(trace[::2], start, end), {})
        self.assertEqual(power_window(trace, start, {**end, "qpc_ns": 2_000_000_000}), {})

    def test_interrupted_or_pilot_campaign_cannot_pass(self):
        with TemporaryDirectory() as temp:
            folder = Path(temp)
            write_json(folder / "campaign.json", {"pilot": False, "config": {"diagnostic": False}})
            with self.assertRaisesRegex(ValueError, "postflight"):
                reduce_campaign(folder, None, None, None)
            write_json(folder / "campaign.json", {"pilot": True, "config": {"diagnostic": False}})
            with self.assertRaisesRegex(ValueError, "pilot"):
                reduce_campaign(folder, None, None, None)

    def test_completed_capture_with_missing_observations_is_pending(self):
        with TemporaryDirectory() as temp:
            folder = Path(temp)
            write_json(folder / "campaign.json", {"pilot": False, "config": {"diagnostic": False}})
            (folder / "complete.txt").touch()
            report = reduce_campaign(folder, None, None, None)
            self.assertFalse(report["all_native_metrics_over_helium_won"])
            self.assertFalse(report["native_non_regression_vs_main"])
            self.assertTrue(report["metrics"])
            self.assertTrue(all(row["status"] == "native validation pending" for row in report["metrics"]))


@unittest.skipUnless(os.getenv("BOWSER_TEST_CHROMIUM"), "set BOWSER_TEST_CHROMIUM for fixture smoke")
class BrowserFixtureTests(unittest.TestCase):
    def test_light_idle_and_demanding_animation(self):
        server = FixtureServer(("127.0.0.1", 0), Handler)
        server.fixtures = {name: (ROOT.parent / "fixtures" / f"{name}.html").read_bytes() for name in FIXTURE}
        server.probe = (ROOT / "probe.js").read_bytes()
        Thread(target=server.serve_forever, daemon=True).start()
        try:
            for fixture, frames in (("light", 0), ("demanding", FRAMES)):
                with self.subTest(fixture=fixture), TemporaryDirectory(ignore_cleanup_errors=True) as profile:
                    token = f"probe-{fixture}"
                    row = dict(token=token, fixture=fixture, workload="animation" if frames else "idle",
                               title_marker=f"Bench-ready-{token}", beacons={})
                    server.attempt, server.control, server.command = row, Event(), {"animation_frames": frames}
                    server.beacon_events = {"ready": Event(), "done": Event()}
                    args = [os.environ["BOWSER_TEST_CHROMIUM"], "--headless", "--no-first-run",
                            "--no-default-browser-check", f"--user-data-dir={profile}",
                            f"http://127.0.0.1:{server.server_port}/{token}/index.html"]
                    with Popen(args, stdout=DEVNULL, stderr=DEVNULL, start_new_session=os.name == "posix") as browser:
                        try:
                            self.assertTrue(server.beacon_events["ready"].wait(15), row)
                            check_ready(row, None)
                            server.control.set()
                            self.assertTrue(server.beacon_events["done"].wait(45), row)
                            ready, done = (row["beacons"][phase] for phase in ("ready", "done"))
                            self.assertTrue(done["visible"])
                            self.assertEqual(ready["viewport"], done["viewport"])
                            times = done.get("timestamps", [])
                            self.assertEqual(len(times), frames)
                            self.assertTrue(all(a < b for a, b in zip(times, times[1:])))
                        finally:
                            server.control.set()
                            if browser.poll() is None:
                                if os.name == "posix":
                                    os.killpg(browser.pid, signal.SIGTERM)
                                else:
                                    browser.terminate()
                            try:
                                browser.wait(timeout=10)
                            except TimeoutExpired:
                                if os.name == "posix":
                                    os.killpg(browser.pid, signal.SIGKILL)
                                else:
                                    browser.kill()
        finally:
            server.shutdown()
            server.server_close()


if __name__ == "__main__":
    unittest.main()
