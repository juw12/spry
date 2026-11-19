
import os, subprocess, sys, time, json, urllib.request
from pathlib import Path
from playwright.sync_api import sync_playwright

home = Path(r"E:\Temp\spry-manual")
out = Path(r"E:\Projects\spry\hone\shots")
out.mkdir(parents=True, exist_ok=True)
env = dict(os.environ, SPRY_HOME=str(home), SPRY_PORT="47991")
proc = subprocess.Popen([r"E:\Projects\spry\target\release\spry.exe", "run", "--no-tray"], env=env)
try:
    time.sleep(7)
    token = (home / "token").read_text().strip()
    url = f"http://127.0.0.1:47991/?t={token}"
    with sync_playwright() as p:
        browser = p.chromium.launch()
        for name, width, height, scheme in [("wide-light", 1280, 900, "light"), ("wide-dark", 1280, 900, "dark"),
                                            ("phone-light", 390, 844, "light"), ("mid-dark", 800, 900, "dark")]:
            page = browser.new_page(viewport={"width": width, "height": height}, color_scheme=scheme)
            errors = []
            page.on("console", lambda m: errors.append(m.text) if m.type == "error" else None)
            page.on("pageerror", lambda e: errors.append(str(e)))
            page.goto(url)
            page.wait_for_timeout(3500)
            page.screenshot(path=str(out / f"{name}.png"), full_page=True)
            overflow = page.evaluate("document.documentElement.scrollWidth - document.documentElement.clientWidth")
            print(name, "errors:", errors[:3], "sideways overflow px:", overflow)
            page.close()
        browser.close()
finally:
    try:
        req = urllib.request.Request(f"http://127.0.0.1:47991/api/quit?t={token}", method="POST")
        urllib.request.urlopen(req, timeout=5).read()
    except Exception as e:
        print("quit failed", e)
        proc.kill()
    proc.wait(timeout=10)
    print("held.json left:", (home / "held.json").exists())
