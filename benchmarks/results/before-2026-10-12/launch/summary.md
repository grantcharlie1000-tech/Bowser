# Launch-to-usable: before

- Executable: C:\Users\charlie\Documents\ChatGPT\Bowser\benchmarks\bin\bowser-before.exe
- SHA-256: a5206312c35e24a6f216f31d283d6fce498ccf8c750c27ef0418ca155e00fcad
- Recorded: 2026-10-12T23:13:59 (git HEAD of checkout: 7979633)
- UI mode override: default (default = whatever the build selects; html-chrome-default builds open the HTML chrome)
- Machine: Intel(R) Core(TM) i5-7300HQ CPU @ 2.50GHz, 4 logical cores, 15.9 GiB, Microsoft Windows 10 Home 10.0.19045, WebView2 runtime 154.0.4258.62
- Power: Power Scheme GUID: 8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c  (High performance); battery status code: 2
- Quiet = system CPU under 35% in the second before launch (the harness waits up to 5 s for that). Other agents/builds share this machine, so non-quiet runs are inflated.
- Cells are `median / p95` in ms. OS file caches are not flushed; cold = fresh profile directory.

## Top metric: process launch to page usable (ms from process_start; usable = page_usable event on the fixture, first navigation_completed on real sites)

| Site | Mode | valid/runs | quiet runs | window_visible | chrome_painted | usable (Bowser) | usable, quiet runs only | outer visible | outer title shown (wall) |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| fixture | cold | 8/8 | 0 | 179.4 / 818.2 | 235.2 / 1003.9 | 8667.5 / 13395.5 | n/a | 586.3 / 2778.7 | 9094.4 / 15390.5 |
| fixture | warm | 8/8 | 0 | 225.3 / 725.8 | 313.3 / 1858.9 | 5099.6 / 6853 | n/a | 744.4 / 16147.6 | 5187.1 / 21958.8 |
| example | cold | 4/4 | 0 | 128 / 289.8 | 183.4 / 343.4 | 3703 / 7295.8 | n/a | 313.2 / 657.7 | 3779.3 / 7402.1 |
| example | warm | 4/4 | 0 | 67.8 / 228.6 | 121.8 / 358.3 | 1577.8 / 3931.9 | n/a | 159.6 / 516.4 | 1578 / 3803 |
| wikipedia | cold | 4/4 | 0 | 223.9 / 403.7 | 328 / 466.3 | 6587.5 / 6951.1 | n/a | 487.3 / 1299.9 | 4619 / 5204.2 |
| wikipedia | warm | 4/4 | 0 | 255.1 / 389.2 | 326.7 / 448.4 | 7589 / 8473.2 | n/a | 470.2 / 594.6 | 4360.8 / 5667.3 |
| github | cold | 4/4 | 0 | 247.8 / 547 | 375.2 / 630.5 | 8879.8 / 12260.3 | n/a | 468.1 / 821.4 | 6433 / 9673 |
| github | warm | 4/4 | 0 | 165.2 / 232.8 | 250.3 / 283.2 | 5592.4 / 7230 | n/a | 489.2 / 1344.9 | 2934.2 / 5620.5 |
| bbc | cold | 4/4 | 0 | 195.8 / 235.4 | 273.7 / 361.4 | 6402.6 / 10050.1 | n/a | 346 / 469.5 | 5339 / 8452.5 |
| bbc | warm | 4/4 | 0 | 181.6 / 399.8 | 244.1 / 499.3 | 4714.8 / 6121.6 | n/a | 492.6 / 774.1 | 3638.5 / 4437.5 |

## Cumulative marks (ms from process_start)

| Site | Mode | engine_started_for_chrome | env_ready | chrome_controller_ready | submitted | content_controller_ready | navigate_called | navigation_started | content_loading | navigation_completed | usable |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| fixture | cold | n/a | 191.6 / 856.1 | 6392.2 / 8097.3 | 179.6 / 818.3 | 6392.2 / 8097.3 | 6393.4 / 8109.6 | 6509.4 / 8627.6 | 8612.2 / 13324.1 | 8654.2 / 13379.2 | 8667.5 / 13395.5 |
| fixture | warm | n/a | 237.4 / 810 | 3244.4 / 5538.8 | 225.4 / 726 | 3244.4 / 5538.8 | 3247.8 / 5541.6 | 3268.6 / 5733.1 | 4862.4 / 6802.9 | 5087.3 / 6839.8 | 5099.6 / 6853 |
| example | cold | n/a | 137.6 / 302.7 | 1665.2 / 2448.4 | 128 / 289.8 | 1665.2 / 2448.4 | 1666.2 / 2452.8 | 1717.6 / 2495.7 | 3575.4 / 7000.6 | 3703 / 7295.8 | 3703 / 7295.8 |
| example | warm | n/a | 77.6 / 238.9 | 828.6 / 1597 | 67.8 / 228.6 | 828.6 / 1597 | 829.3 / 1597.7 | 864 / 1649.5 | 1475.2 / 3559.4 | 1577.8 / 3931.9 | 1577.8 / 3931.9 |
| wikipedia | cold | n/a | 237.2 / 420.5 | 2039.3 / 2273.3 | 224 / 403.7 | 2039.3 / 2273.3 | 2041.1 / 2274.5 | 2082.4 / 2316 | 4191.5 / 4481.7 | 6587.5 / 6951.1 | 6587.5 / 6951.1 |
| wikipedia | warm | n/a | 267.5 / 405.4 | 2016.4 / 3035.5 | 255.2 / 389.2 | 2016.4 / 3035.5 | 2018.4 / 3038.1 | 2096.8 / 3179.4 | 4079.4 / 5161 | 7589 / 8473.2 | 7589 / 8473.2 |
| github | cold | n/a | 261.6 / 564.3 | 4816.2 / 7396.8 | 247.8 / 547.1 | 4816.2 / 7396.8 | 4817.2 / 7418 | 4864.3 / 7534.8 | 6105.7 / 9222.4 | 8879.8 / 12260.3 | 8879.8 / 12260.3 |
| github | warm | n/a | 174.8 / 242.3 | 1261.9 / 1687 | 165.2 / 232.8 | 1261.9 / 1687 | 1264.8 / 1687.8 | 1391.3 / 1812.9 | 2506.4 / 4010.5 | 5592.4 / 7230 | 5592.4 / 7230 |
| bbc | cold | n/a | 212.6 / 250.5 | 2328.4 / 2825.8 | 195.8 / 235.5 | 2328.4 / 2825.8 | 2329.1 / 2828.3 | 2428.9 / 2890.4 | 4912.4 / 8094.8 | 6402.6 / 10050.1 | 6402.6 / 10050.1 |
| bbc | warm | n/a | 197 / 411.7 | 2181.2 / 2602.4 | 181.6 / 399.8 | 2181.2 / 2602.4 | 2182.2 / 2606.4 | 2391.6 / 2674.7 | 3161 / 4234.6 | 4714.8 / 6121.6 | 4714.8 / 6121.6 |

## Phase durations (ms)

| Site | Mode | window_visible | chrome_paint | env_create | chrome_controller | content_controller | to_submitted | submit_to_navigate | navigate_to_started | started_to_loading | loading_to_completed | completed_to_usable | submit_to_usable |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| fixture | cold | 179.4 / 818.2 | 55.2 / 284 | 11.3 / 43.6 | 6066.8 / 7846.2 | 6066.8 / 7846.2 | 179.5 / 818.3 | 6116 / 7860.6 | 96 / 770.9 | 1861 / 6857.3 | 40.2 / 56.3 | 11.1 / 17.8 | 8464.8 / 12577.2 |
| fixture | warm | 225.2 / 725.6 | 98.4 / 1133.1 | 10.9 / 101.8 | 3019.9 / 4755.1 | 3019.9 / 4755.1 | 225.2 / 725.8 | 3031.2 / 4818.4 | 81.6 / 358.1 | 877.6 / 2105.4 | 38.2 / 278.4 | 11.4 / 14.8 | 4850.6 / 6179 |
| example | cold | 127.9 / 289.7 | 54.7 / 55.9 | 9 / 12.2 | 1535.6 / 2145.5 | 1535.6 / 2145.5 | 128 / 289.8 | 1546.6 / 2163 | 47.4 / 253.7 | 1857.8 / 4505 | 127.6 / 295.2 | n/a | 3575 / 7006 |
| example | warm | 67.7 / 228.5 | 55 / 129.8 | 8.3 / 10 | 750.6 / 1360.5 | 750.6 / 1360.5 | 67.7 / 228.5 | 761.6 / 1371.2 | 46.5 / 71.5 | 611.2 / 1910.8 | 127.2 / 375.3 | n/a | 1510 / 3703.3 |
| wikipedia | cold | 223.9 / 403.6 | 61.1 / 136.2 | 14.8 / 16.7 | 1801.7 / 1852.4 | 1801.7 / 1852.4 | 223.9 / 403.6 | 1817.2 / 1870.7 | 41.6 / 42.8 | 2107.4 / 2447.9 | 2468.1 / 2745 | n/a | 6231.2 / 6837.5 |
| wikipedia | warm | 255.1 / 389.1 | 60.7 / 82 | 12.5 / 15.2 | 1749.8 / 2679.9 | 1749.8 / 2679.9 | 255.1 / 389.1 | 1766.8 / 2698.2 | 99.4 / 162.6 | 1892.2 / 2173.4 | 3333 / 3581.7 | n/a | 7298.6 / 8133.5 |
| github | cold | 247.8 / 547 | 80.2 / 159 | 13.9 / 16.5 | 4419.2 / 7153.3 | 4419.2 / 7153.3 | 247.8 / 547 | 4437.4 / 7190 | 70.8 / 119.6 | 1319.8 / 1771.2 | 3212.8 / 3641.1 | n/a | 8425.1 / 12032.4 |
| github | warm | 165.2 / 232.7 | 50.8 / 112.1 | 8.8 / 9.4 | 1131.1 / 1473.1 | 1131.1 / 1473.1 | 165.2 / 232.8 | 1142.2 / 1483.4 | 115.3 / 140.7 | 1173.2 / 2235.8 | 2338.8 / 4500.8 | n/a | 5438.2 / 7103.8 |
| bbc | cold | 195.7 / 235.4 | 73 / 150.3 | 14.6 / 15.7 | 2101 / 2595 | 2101 / 2595 | 195.7 / 235.4 | 2117.8 / 2614.6 | 48.8 / 134.1 | 2652.2 / 5257.9 | 1490.2 / 1955.3 | n/a | 6191.2 / 9845 |
| bbc | warm | 181.5 / 399.8 | 63.2 / 99.4 | 11.9 / 15.2 | 1984 / 2489.3 | 1984 / 2489.3 | 181.5 / 399.8 | 2000.6 / 2507 | 97.6 / 257.1 | 1009 / 1580 | 1561.2 / 1939.1 | n/a | 4533.2 / 6013.4 |

Phase definitions: window_visible = process_start to window_visible; chrome_paint = window_visible to chrome_painted; env_create = env_requested to env_ready; chrome_controller = chrome_controller_requested to chrome_controller_ready; content_controller = content_controller_requested to content_controller_ready; to_submitted = process_start to submitted; submit_to_navigate = submitted to navigate_called; navigate_to_started = navigate_called to navigation_started; started_to_loading = navigation_started to content_loading; loading_to_completed = content_loading to navigation_completed; completed_to_usable = navigation_completed to page_usable; submit_to_usable = submitted to usable.
env_requested/env_ready = first webview_environment_requested/result. chrome_controller = first webview_controller_requested/result (HTML chrome toolbar controller when present). content_controller = last controller request/result before the first navigate_called.
Raw rows: runs.csv. Event logs: logs\.
