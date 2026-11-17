import { launch, instrumentedContext } from "./lib.mjs";
import { startServer } from "../fixtures/server.mjs";
const srv = await startServer();
const browser = await launch();
for (const [name, scheme, host, path, vp] of [["x-dark", "dark", "x.com", "/home", { width: 700, height: 760 }], ["x-light", "light", "x.com", "/home", { width: 700, height: 760 }], ["google", "light", "www.google.com", "/search?q=weather", { width: 700, height: 420 }]]) {
  const ctx = await instrumentedContext(browser, { config: {}, detection: "real", probe: false, viewport: vp });
  const page = await ctx.newPage();
  await page.emulateMedia({ colorScheme: scheme });
  await page.goto(`http://${host}:${srv.portA}${path}`);
  await new Promise((r) => setTimeout(r, 700));
  await page.screenshot({ path: `out/shot-${name}.png` });
  await ctx.close();
}
await browser.close(); await srv.close();
