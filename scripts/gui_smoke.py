#!/usr/bin/env python3
"""Exercise the real Tauri webview, Rust commands, HTTP protocol, and installer.

Run `cargo build --features e2e` and then `python3 scripts/gui_smoke.py` in a
desktop session with WebKitWebDriver installed. No Python packages are needed.
All credentials, downloads, and installations in this test are synthetic.
"""

import base64
import io
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import tempfile
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
import zipfile
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

ROOT = Path(__file__).resolve().parents[1]
RESULTS = ROOT / "test-results"
PRODUCT = "ZygorGuidesViewer"
TOKEN = "fixture-token+&?"


class Fixture(BaseHTTPRequestHandler):
    calls = []
    invalid_archive = False
    slow_download = False
    failures = []

    def log_message(self, *_):
        pass

    def respond(self, data, status=200, content_type="application/json"):
        if not isinstance(data, bytes):
            data = json.dumps(data).encode()
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        try:
            if self.slow_download:
                self.wfile.write(data[:4])
                self.wfile.flush()
                time.sleep(2)
                self.wfile.write(data[4:])
            else:
                self.wfile.write(data)
        except (BrokenPipeError, ConnectionResetError):
            pass

    def do_GET(self):
        try:
            self.handle_api()
        except Exception as exc:
            self.failures.append(str(exc))
            self.respond({"error": "fixture assertion failed"}, 500)

    def do_POST(self):
        self.do_GET()

    def handle_api(self):
        q = dict(urllib.parse.parse_qsl(urllib.parse.urlsplit(self.path).query))
        action = q.get("do")
        self.calls.append(action)
        assert self.headers["X-Zygor-Api"] == "4", "API version header"
        assert self.headers["X-Zygor-UV"] == "4.13.2", "Compatibility version header"
        assert q["full"] == "1", "Full response request"
        if action == "validate":
            if "user" in q:
                expected = "Qö" + base64.b64encode("päss+word &?".encode()).decode()
                if q["user"] != "fixture-user" or q.get("pass") != expected:
                    self.respond({"code": "userpass", "text": "Invalid login"}, 401)
                    return
            else:
                assert q.get("token") == TOKEN, "Session token was not encoded correctly"
            self.respond({"success": True, "user": {"login": "fixture-user", "token": TOKEN},
                          "access": {"level": "elite", "status": "active"}})
        elif action == "list_packages":
            assert q["token"] == TOKEN
            self.respond({"success": True, "packages": [{"game": "wow", "name": PRODUCT,
                          "ver": "9.6.101", "status": {"available": True, "hidden": False}}]})
        elif action == "comp":
            assert self.command == "POST"
            assert q["token"] == TOKEN and q["package"] == PRODUCT
            body = self.rfile.read(int(self.headers.get("Content-Length", 0))).decode()
            form = dict(urllib.parse.parse_qsl(body, keep_blank_values=True))
            assert "list" in form and f"F {PRODUCT}/" in form["list"], "Missing file manifest"
            output = io.BytesIO()
            with zipfile.ZipFile(output, "w", zipfile.ZIP_DEFLATED) as z:
                if self.invalid_archive:
                    z.writestr("../escape.lua", "invalid archive")
                else:
                    z.writestr(f"{PRODUCT}/{PRODUCT}.toc", "## Version: 9.6\n")
                    z.writestr(f"{PRODUCT}/Ver.lua", "$Revision: 101 $")
                    z.writestr(f"{PRODUCT}/Guide.lua", "new guide")
                    z.writestr(".zygor-meta", json.dumps({"DELETE": [f"{PRODUCT}/obsolete.lua"]}))
            self.respond(output.getvalue(), content_type="application/zip")
        elif action == "license":
            assert q["token"] == TOKEN
            self.respond(b"-- synthetic fixture licence", content_type="text/plain")
        else:
            self.respond({"error": "Unknown fixture action"}, 400)


class Driver:
    def __init__(self, port):
        self.base = f"http://127.0.0.1:{port}"
        self.session = ""

    def request(self, method, path, data=None):
        body = None if data is None else json.dumps(data).encode()
        req = urllib.request.Request(self.base + path, data=body, method=method,
                                     headers={"Content-Type": "application/json"})
        try:
            with urllib.request.urlopen(req, timeout=35) as response:
                result = json.load(response)
        except urllib.error.HTTPError as exc:
            raise RuntimeError(exc.read().decode()) from exc
        value = result.get("value")
        if isinstance(value, dict) and value.get("error"):
            raise RuntimeError(value)
        return value

    def command(self, method, path, data=None):
        return self.request(method, f"/session/{self.session}{path}", data)

    def js(self, script, *args):
        return self.command("POST", "/execute/sync", {"script": script, "args": list(args)})

    def element(self, selector):
        result = self.command("POST", "/element", {"using": "css selector", "value": selector})
        return result["element-6066-11e4-a52e-4f735466cecf"]

    def click(self, selector):
        self.command("POST", f"/element/{self.element(selector)}/click", {})

    def fill(self, selector, value):
        element = self.element(selector)
        self.command("POST", f"/element/{element}/clear", {})
        self.command("POST", f"/element/{element}/value", {"text": value})

    def wait(self, predicate, timeout=25):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if self.js("return " + predicate):
                return
            time.sleep(0.1)
        text = self.js("return document.body.innerText")
        raise AssertionError(f"Timed out waiting for {predicate}\n{text}")

    def screenshot(self, name):
        RESULTS.mkdir(exist_ok=True)
        image = self.command("GET", "/screenshot")
        (RESULTS / name).write_bytes(base64.b64decode(image))


def free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def run():
    binary = ROOT / "target/debug/openzygor"
    assert binary.exists(), "Run cargo build --features e2e first"
    assert subprocess.run([binary, "--e2e-check"]).returncode == 0, "This is a production binary. Run cargo build --features e2e first."
    assert shutil.which("WebKitWebDriver"), "Install WebKitWebDriver / webkit2gtk-4.1"
    RESULTS.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="openzygor-e2e-") as tmp:
        tmp = Path(tmp)
        game = tmp / "World of Warcraft/_retail_"
        addon = game / "Interface/AddOns" / PRODUCT
        (addon / "Custom").mkdir(parents=True)
        (game / "Wow.exe").write_bytes(b"synthetic test executable")
        (addon / f"{PRODUCT}.toc").write_text("## Version: 9.6\n")
        (addon / "Ver.lua").write_text("$Revision: 100 $")
        (addon / "Guide.lua").write_text("old guide")
        (addon / "obsolete.lua").write_text("obsolete")
        (addon / "Custom/MyGuide.lua").write_text("custom guide")
        config = tmp / "config"
        config.mkdir()
        (config / "settings.json").write_text(json.dumps({"region": "eu", "remember": False,
            "installations": [{"id": "fixture-retail", "game": "wow", "path": str(game)}]}))
        server = ThreadingHTTPServer(("127.0.0.1", 0), Fixture)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        env = dict(os.environ, OPENZYGOR_CONFIG_DIR=str(config), TAURI_WEBVIEW_AUTOMATION="true",
                   OPENZYGOR_TEST_ENDPOINT=f"http://127.0.0.1:{server.server_port}/packager.php")
        port = free_port()
        driver = Driver(port)
        with (RESULTS / "webdriver.log").open("w") as logfile:
            process = subprocess.Popen(["WebKitWebDriver", f"--port={port}"], env=env,
                                       stdout=logfile, stderr=subprocess.STDOUT)
            try:
                for _ in range(50):
                    try:
                        driver.request("GET", "/status")
                        break
                    except (OSError, RuntimeError):
                        if process.poll() is not None:
                            raise RuntimeError("WebKitWebDriver exited; see test-results/webdriver.log")
                        time.sleep(0.1)
                result = driver.request("POST", "/session", {"capabilities": {"alwaysMatch": {
                    "webkitgtk:browserOptions": {"binary": str(binary), "args": []}}}})
                driver.session = result["sessionId"]
                driver.wait("document.querySelector('#login-form') !== null")
                driver.screenshot("login.png")
                driver.fill("#username", "fixture-user")
                driver.fill("#password", "incorrect")
                driver.click("[name=remember]")
                driver.click("#login-form button[type=submit]")
                driver.wait("document.querySelector('[role=alert]') !== null")
                assert "rejected" in driver.js("return document.querySelector('[role=alert]').textContent")
                driver.fill("#password", "päss+word &?")
                # Keep this test keyring-free even if a UI default changes.
                if driver.js("return document.querySelector('[name=remember]').checked"):
                    driver.click("[name=remember]")
                driver.click("#login-form button[type=submit]")
                driver.wait("document.querySelector('[data-action=install]') !== null")
                driver.screenshot("guides-before.png")
                driver.click("[data-action=install]")
                driver.wait("document.querySelector('.banner.success') !== null")
                assert (addon / "Guide.lua").read_text() == "new guide"
                assert (addon / "Custom/MyGuide.lua").read_text() == "custom guide"
                assert not (addon / "obsolete.lua").exists()
                assert (addon / "Licence.lua").read_text() == "-- synthetic fixture licence"
                driver.screenshot("guides-installed.png")
                # Repair uses the same checksum flow even when versions match.
                (addon / "Guide.lua").write_text("damaged")
                driver.click("[data-action=install]")
                driver.wait("document.querySelector('.banner.success') !== null")
                assert (addon / "Guide.lua").read_text() == "new guide"
                Fixture.invalid_archive = True
                driver.click("[data-action=install]")
                driver.wait("document.querySelector('[role=alert]') !== null")
                assert (addon / "Guide.lua").read_text() == "new guide"
                assert not (game / "Interface/AddOns/escape.lua").exists()
                Fixture.invalid_archive = False
                Fixture.slow_download = True
                driver.click("[data-action=install]")
                driver.wait("document.querySelector('[data-action=cancel]') !== null")
                driver.click("[data-action=cancel]")
                driver.wait("document.querySelector('[role=alert]') !== null")
                Fixture.slow_download = False
                assert (addon / "Guide.lua").read_text() == "new guide"
                driver.click("[data-action=uninstall]")
                driver.wait("document.querySelector('#confirm-dialog').open")
                driver.click("#confirm-yes")
                driver.wait("document.querySelector('.banner.success') !== null")
                assert not addon.exists()
                assert (game / "Interface/AddOns/.openzygor-ZygorGuidesViewer/uninstalled/Guide.lua").exists()
                driver.click("[data-page=directories]")
                driver.wait("document.querySelector('#folder-form') !== null")
                driver.screenshot("folders.png")
                driver.click("[data-page=settings]")
                driver.wait("document.querySelector('#settings-form') !== null")
                driver.screenshot("preferences.png")
                driver.click("[data-action=logout]")
                driver.wait("document.querySelector('.account-chip').textContent.includes('Not signed in')")
                stored = (config / "settings.json").read_text()
                assert TOKEN not in stored and "päss" not in stored and '"token"' not in stored
                assert not Fixture.failures, Fixture.failures
                print("PASS: native login errors, login, HTTP protocol, update, repair, archive rejection, cancellation, uninstall, folders, preferences, and logout")
                print(f"Screenshots and WebDriver log: {RESULTS}")
            except Exception:
                if driver.session:
                    try:
                        driver.screenshot("failure.png")
                    except Exception:
                        pass
                raise
            finally:
                if driver.session:
                    try:
                        driver.request("DELETE", f"/session/{driver.session}")
                    except Exception:
                        pass
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                server.shutdown()
                server.server_close()


if __name__ == "__main__":
    run()
