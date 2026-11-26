"""Compare local browser processes while loading the same loopback page."""

import argparse
import ctypes as ct
from ctypes import wintypes as wt
from dataclasses import dataclass, field
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
import stat
from pathlib import Path
import statistics
import subprocess
import tempfile
import threading
import time
from urllib.parse import parse_qs, urlsplit
import uuid

ROOT = Path(__file__).resolve().parent
MIB = 1024**2
PROCESS_QUERY_INFORMATION = 0x0400
PROCESS_VM_READ = 0x0010
TH32CS_SNAPPROCESS = 0x00000002


class ProcessEntry(ct.Structure):
    _fields_ = [
        ("size", wt.DWORD), ("usage", wt.DWORD), ("pid", wt.DWORD),
        ("heap", ct.c_size_t), ("module", wt.DWORD), ("threads", wt.DWORD),
        ("parent", wt.DWORD), ("priority", wt.LONG), ("flags", wt.DWORD),
        ("exe", wt.WCHAR * 260),
    ]


class MemoryCounters(ct.Structure):
    _fields_ = [
        ("size", wt.DWORD), ("faults", wt.DWORD),
        ("peak_working", ct.c_size_t), ("working", ct.c_size_t),
        ("peak_paged_pool", ct.c_size_t), ("paged_pool", ct.c_size_t),
        ("peak_nonpaged_pool", ct.c_size_t), ("nonpaged_pool", ct.c_size_t),
        ("commit", ct.c_size_t), ("peak_commit", ct.c_size_t),
        ("private_commit", ct.c_size_t), ("private_working", ct.c_size_t),
        ("shared_commit", ct.c_uint64),
    ]


class WindowsProcesses:
    def __init__(self):
        self.kernel = ct.WinDLL("kernel32", use_last_error=True)
        self.psapi = ct.WinDLL("psapi", use_last_error=True)
        self.kernel.CreateToolhelp32Snapshot.argtypes = (wt.DWORD, wt.DWORD)
        self.kernel.CreateToolhelp32Snapshot.restype = wt.HANDLE
        for name in ("Process32FirstW", "Process32NextW"):
            function = getattr(self.kernel, name)
            function.argtypes = (wt.HANDLE, ct.POINTER(ProcessEntry))
            function.restype = wt.BOOL
        self.kernel.OpenProcess.argtypes = (wt.DWORD, wt.BOOL, wt.DWORD)
        self.kernel.OpenProcess.restype = wt.HANDLE
        self.kernel.CloseHandle.argtypes = (wt.HANDLE,)
        self.kernel.CloseHandle.restype = wt.BOOL
        self.psapi.GetProcessMemoryInfo.argtypes = (
            wt.HANDLE, ct.POINTER(MemoryCounters), wt.DWORD,
        )
        self.psapi.GetProcessMemoryInfo.restype = wt.BOOL

    def sample(self, root):
        # Child processes can exit between the snapshot and the memory query.
        for attempt in range(5):
            try:
                return self.sample_tree(root)
            except OSError as error:
                if error.winerror not in (6, 87, 299) or attempt == 4:
                    raise
                time.sleep(0.15)

    def sample_tree(self, root):
        snapshot = self.kernel.CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)
        if snapshot == ct.c_void_p(-1).value:
            raise ct.WinError(ct.get_last_error())
        entries = {}
        try:
            row = ProcessEntry(size=ct.sizeof(ProcessEntry))
            present = self.kernel.Process32FirstW(snapshot, ct.byref(row))
            if not present:
                raise ct.WinError(ct.get_last_error())
            while present:
                entries[row.pid] = (row.parent, row.exe)
                present = self.kernel.Process32NextW(snapshot, ct.byref(row))
        finally:
            self.kernel.CloseHandle(snapshot)
        if root not in entries:
            raise RuntimeError(f"O processo principal {root} saiu antes da medição.")
        tree = {root}
        while children := {pid for pid, (parent, _) in entries.items() if parent in tree} - tree:
            tree.update(children)
        private_ram = private_commit = 0
        names = []
        for pid in sorted(tree):
            process = self.kernel.OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, False, pid)
            if not process:
                raise ct.WinError(ct.get_last_error())
            try:
                counters = MemoryCounters(size=ct.sizeof(MemoryCounters))
                if not self.psapi.GetProcessMemoryInfo(process, ct.byref(counters), counters.size):
                    raise ct.WinError(ct.get_last_error())
                private_ram += counters.private_working
                private_commit += counters.private_commit
                names.append(entries[pid][1])
            finally:
                self.kernel.CloseHandle(process)
        return {
            "private_ram_mib": round(private_ram / MIB, 2),
            "private_commit_mib": round(private_commit / MIB, 2),
            "processes": names,
        }


@dataclass
class Fixture:
    filtering: bool = False
    ready: threading.Event = field(default_factory=threading.Event)
    result: dict = field(default_factory=dict)
    resource_requests: set = field(default_factory=set)


PAGE = """<!doctype html>
<html lang="pt-BR"><meta charset="utf-8"><title>Brisa | página de teste</title>
<style>body{font:16px sans-serif;margin:40px}main{max-width:70ch}article{border-bottom:1px solid #ddd}</style>
<main><h1>Página de teste local</h1><div id="articles"></div></main>
<script>
const articles = document.querySelector('#articles');
for (let i = 1; i <= 120; i++) {
  const article = document.createElement('article');
  const title = document.createElement('h2');
  title.textContent = 'Artigo ' + i;
  const paragraph = document.createElement('p');
  paragraph.textContent = 'Conteúdo servido neste computador. '.repeat(12);
  article.append(title, paragraph);
  articles.append(article);
}
addEventListener('load', () => {
  const img = document.querySelector('#ad');
  const result = new URLSearchParams({
    articles: articles.children.length,
    ad_loaded: String(Boolean(img?.naturalWidth)),
    tracker_ran: String(Boolean(window.fixtureTracker)),
    popup_attempted: String(Boolean(window.fixturePopupAttempted)),
  });
  fetch('result?' + result);
});
</script>
"""
IMAGE = b'<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><rect width="16" height="16"/></svg>'


def handler_for(fixtures):
    class Handler(BaseHTTPRequestHandler):
        def do_GET(self):
            url = urlsplit(self.path)
            _, _, suffix = url.path.partition("/")
            key, _, resource = suffix.partition("/")
            fixture = fixtures.get(key)
            if fixture is None:
                self.send_error(404)
                return
            if resource == "":
                page = PAGE
                if fixture.filtering:
                    page += (
                        '<img id="ad" src="ad.svg" alt=""><script src="tracker.js"></script>'
                        '<script>window.fixturePopupAttempted = true; window.open("unexpected-popup");</script>'
                    )
                self.reply("text/html; charset=utf-8", page.encode())
            elif resource == "ad.svg":
                fixture.resource_requests.add(resource)
                self.reply("image/svg+xml", IMAGE)
            elif resource == "tracker.js":
                fixture.resource_requests.add(resource)
                self.reply("text/javascript", b"window.fixtureTracker = true;")
            elif resource == "unexpected-popup":
                fixture.resource_requests.add(resource)
                self.reply("text/plain", b"Popup de teste")
            elif resource == "result":
                fixture.result = {key: values[0] for key, values in parse_qs(url.query).items()}
                self.reply("text/plain", b"OK")
                fixture.ready.set()
            else:
                self.send_error(404)

        def reply(self, content_type, body):
            self.send_response(200)
            self.send_header("Content-Type", content_type)
            self.send_header("Cache-Control", "no-store")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, *_):
            pass

    return Handler


def run_browser(name, executable, server, fixtures, processes, filtering=False):
    tag = uuid.uuid4().hex
    fixture = Fixture(filtering=filtering)
    fixtures[tag] = fixture
    url = f"http://127.0.0.1:{server.server_port}/{tag}/"
    with tempfile.TemporaryDirectory(prefix="brisa-benchmark-", ignore_cleanup_errors=True) as work:
        profile = Path(work) / "profile"
        if name.startswith("Brisa"):
            arguments = ["--url", url, "--profile", str(profile)]
            if filtering:
                rules = Path(work) / "fixture.txt"
                rules.write_text("/ad.svg|$image\n/tracker.js|$script\n", encoding="utf-8")
                arguments.extend(["--rules", str(rules)])
        else:
            arguments = [f"--user-data-dir={profile}", "--no-first-run", "--no-default-browser-check", url]
        browser = subprocess.Popen([str(executable), *arguments], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        try:
            deadline = time.monotonic() + 35
            while not fixture.ready.wait(0.25):
                if browser.poll() is not None:
                    raise RuntimeError(f"{name} saiu antes do carregamento, código {browser.returncode}.")
                if time.monotonic() >= deadline:
                    raise RuntimeError(f"{name} não concluiu o carregamento da página de teste.")
            if fixture.result.get("articles") != "120":
                raise RuntimeError(f"{name} não renderizou a página de teste.")
            if filtering:
                return {**fixture.result, "requests_reaching_server": sorted(fixture.resource_requests)}
            time.sleep(3)
            samples = []
            for _ in range(3):
                samples.append(processes.sample(browser.pid))
                time.sleep(0.6)
            renderer = "msedgewebview2.exe" if name.startswith("Brisa") else executable.name.lower()
            if any(sum(p.lower() == renderer for p in sample["processes"]) < 2 for sample in samples):
                raise RuntimeError(f"Árvore de processos incompleta para {name}.")
            return {
                "private_ram_mib": statistics.median(s["private_ram_mib"] for s in samples),
                "private_commit_mib": statistics.median(s["private_commit_mib"] for s in samples),
                "profile_mib": profile_mib(profile),
                "samples": samples,
            }
        finally:
            if browser.poll() is None:
                taskkill = Path(os.environ["SystemRoot"]) / "System32" / "taskkill.exe"
                subprocess.run(
                    [str(taskkill), "/PID", str(browser.pid), "/T", "/F"],
                    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False,
                )
                if browser.poll() is None:
                    browser.terminate()
                browser.wait(timeout=10)
            fixtures.pop(tag, None)


def profile_mib(profile):
    size = 0
    for directory, _, files in os.walk(profile):
        for filename in files:
            try:
                item = (Path(directory) / filename).stat(follow_symlinks=False)
            except FileNotFoundError:
                continue
            if stat.S_ISREG(item.st_mode):
                size += item.st_size
    return round(size / MIB, 2)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--brisa", type=Path, default=ROOT / "target/release/brisa-06.exe")
    parser.add_argument("--baseline", type=Path, help="Executável anterior do Brisa para medir no mesmo cenário.")
    parser.add_argument("--chrome", type=Path, default=Path(r"C:\Program Files\Google\Chrome\Application\chrome.exe"))
    parser.add_argument("--brave", type=Path, default=Path(r"C:\Program Files\BraveSoftware\Brave-Browser\Application\brave.exe"))
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--output", type=Path, default=ROOT / "evidence/benchmark-v0.6.json")
    args = parser.parse_args()
    if args.runs < 1:
        parser.error("--runs deve ser maior ou igual a 1")
    browsers = {"Brisa": args.brisa, "Chrome": args.chrome, "Brave": args.brave}
    if args.baseline:
        browsers["Brisa anterior"] = args.baseline
    for path in browsers.values():
        if not path.is_file():
            parser.error(f"Executável ausente: {path}")
    processes = WindowsProcesses()
    fixtures = {}
    with ThreadingHTTPServer(("127.0.0.1", 0), handler_for(fixtures)) as server:
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            unfiltered = run_browser("Chrome", args.chrome, server, fixtures, processes, filtering=True)
            filtered = run_browser("Brisa", args.brisa, server, fixtures, processes, filtering=True)
            if (
                unfiltered["requests_reaching_server"] != ["ad.svg", "tracker.js"]
                or unfiltered["ad_loaded"] != "true"
                or unfiltered["tracker_ran"] != "true"
                or filtered["requests_reaching_server"]
                or filtered["ad_loaded"] != "false"
                or filtered["tracker_ran"] != "false"
                or filtered["popup_attempted"] != "true"
            ):
                raise RuntimeError(f"O teste de bloqueio falhou. Chrome={unfiltered} Brisa={filtered}")
            print("O anúncio, o rastreador e o pop-up automáticos foram bloqueados pelo Brisa.", flush=True)
            results = {name: [] for name in browsers}
            ordered_browsers = list(browsers.items())
            for trial in range(args.runs):
                offset = trial % len(ordered_browsers)
                for name, executable in ordered_browsers[offset:] + ordered_browsers[:offset]:
                    sample = run_browser(name, executable, server, fixtures, processes)
                    results[name].append(sample)
                    print(f"{name}: {sample['private_ram_mib']:.1f} MiB de RAM privada", flush=True)
        finally:
            server.shutdown()
            thread.join()
    metrics = ("private_ram_mib", "private_commit_mib", "profile_mib")
    report = {
        "fixture": "1 página local, 120 artigos no DOM, perfil novo por execução",
        "filtering": {"Chrome": unfiltered, "Brisa": filtered},
        "median": {
            name: {metric: statistics.median(sample[metric] for sample in runs) for metric in metrics}
            for name, runs in results.items()
        },
        "runs": results,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(json.dumps(report["median"], indent=2))


if __name__ == "__main__":
    main()
