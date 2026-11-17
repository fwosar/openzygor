#!/usr/bin/env python3
"""Verify activity history across native application restarts.

Build with `cargo build --features e2e`, then run this script in a desktop session.
All settings and game folders are temporary. No Zygor account is used.
"""

import json
import os
from pathlib import Path
import subprocess
import tempfile
import time

from gui_smoke import Driver, RESULTS, ROOT, free_port


def run():
    binary = ROOT / "target/debug/openzygor"
    assert subprocess.run([binary, "--e2e-check"]).returncode == 0, "Build with --features e2e first"
    RESULTS.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="openzygor-activity-") as temporary:
        root = Path(temporary)
        game = root / "World of Warcraft/_retail_"
        game.mkdir(parents=True)
        (game / "Wow.exe").write_bytes(b"fixture")
        config = root / "config"
        config.mkdir()
        (config / "settings.json").write_text(json.dumps({
            "region": "eu", "remember": False,
            "installations": [{"id": "fixture", "game": "wow", "path": str(game)}],
        }))
        port = free_port()
        driver = Driver(port)
        env = dict(os.environ, OPENZYGOR_CONFIG_DIR=str(config),
                   XDG_DATA_HOME=str(root / "data"), XDG_CACHE_HOME=str(root / "cache"),
                   TAURI_WEBVIEW_AUTOMATION="true",
                   OPENZYGOR_TEST_ENDPOINT=f"http://127.0.0.1:{free_port()}/unused")

        def launch():
            result = driver.request("POST", "/session", {"capabilities": {"alwaysMatch": {
                "webkitgtk:browserOptions": {"binary": str(binary), "args": []}}}})
            driver.session = result["sessionId"]
            driver.wait("document.querySelector('#login-form') !== null")

        def restart():
            driver.request("DELETE", f"/session/{driver.session}")
            driver.session = ""
            launch()

        def entries():
            return driver.js("return Array.from(document.querySelectorAll('.log-entry')).map(e => ({message: e.querySelector('p').textContent, failed: e.classList.contains('failed'), time: e.querySelector('time').textContent}))")

        with (RESULTS / "activity-webdriver.log").open("w") as logfile:
            process = subprocess.Popen(["WebKitWebDriver", f"--port={port}"], env=env,
                                       stdout=logfile, stderr=subprocess.STDOUT)
            try:
                for _ in range(50):
                    try:
                        driver.request("GET", "/status")
                        break
                    except OSError:
                        time.sleep(0.1)
                launch()
                driver.click("[data-page=settings]")
                driver.click("#settings-form button[type=submit]")
                driver.wait("document.querySelector('.banner.success') !== null")
                driver.click("[data-page=directories]")
                driver.fill("#directory", str(root / "missing-game"))
                driver.click("#folder-form button[type=submit]")
                driver.wait("document.querySelector('[role=alert]') !== null")
                driver.click("[data-page=activity]")
                before = entries()
                assert len(before) == 2 and before[0]["failed"] and not before[1]["failed"], before
                driver.screenshot("activity-before-restart.png")
                restart()
                driver.click("[data-page=activity]")
                after = entries()
                driver.screenshot("activity-after-restart.png")
                assert after == before, f"Activity lost after restart: before={before}, after={after}"
                driver.click("[data-action=clear-log]")
                driver.wait("document.querySelector('.log-entry') === null")
                restart()
                driver.click("[data-page=activity]")
                assert entries() == [], "Cleared history returned after restart"
                print("PASS: success and error activity survive native app restarts; clearing remains cleared")
            finally:
                if driver.session:
                    driver.request("DELETE", f"/session/{driver.session}")
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()


if __name__ == "__main__":
    run()
