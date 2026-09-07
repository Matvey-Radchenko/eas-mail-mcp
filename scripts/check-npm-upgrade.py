#!/usr/bin/env python3
"""Mac-only isolated package checks; never publishes or changes the user's install.

Positive production MCP checks require explicit --existing-keychain-read. They
use zero configured accounts and only inspect tools/account/journal metadata.
The existing credential item must already exist; this script never creates one.
"""

from __future__ import annotations

import argparse
import asyncio
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import sqlite3
import subprocess
import tarfile
import tempfile


ROOT = Path(__file__).resolve().parent.parent
FIXTURE = ROOT / "crates/app/src/runtime/production/upgrade/fixture.rs"


class McpCheckFailed(RuntimeError):
    def __init__(self, report: dict) -> None:
        super().__init__(report["reason"])
        self.report = report


def frozen(name: str) -> str:
    match = re.search(
        rf'pub\(super\) const {name}: &str = (?:r#"(.*?)"#|"(.*?)");',
        FIXTURE.read_text(), re.DOTALL,
    )
    if not match:
        raise RuntimeError("frozen upgrade fixture is missing")
    return next(value for value in match.groups() if value is not None)


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def packaged_digest(archive: Path) -> str:
    with tarfile.open(archive) as package:
        binary = package.extractfile("package/bin/eas-mail-mcp")
        if binary is None:
            raise RuntimeError("native archive is missing its executable")
        return hashlib.sha256(binary.read()).hexdigest()


def child_environment(root: Path) -> dict[str, str]:
    # Child-process home isolation does not reassign the shell's HOME or global npm prefix.
    home = root / "home"
    home.mkdir(parents=True, exist_ok=True)
    return {
        "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
        "HOME": str(home),
        "TMPDIR": str(root),
        "LANG": "en_US.UTF-8",
    }


def command(args: list[str | Path], env: dict[str, str], expected: int = 0) -> str:
    result = subprocess.run(
        [str(value) for value in args], env=env, cwd=ROOT,
        capture_output=True, text=True, timeout=180,
    )
    if result.returncode != expected:
        details = result.stderr.strip() if str(args[0]) == "npm" else ""
        raise RuntimeError(f"command exit mismatch: expected {expected}, got {result.returncode}\n{details}")
    return result.stdout


def npm(args: list[str | Path], state: Path, env: dict[str, str]) -> str:
    user_config = state / "user-npmrc"
    global_config = state / "global-npmrc"
    user_config.touch()
    global_config.touch()
    return command([
        "npm", *args, "--ignore-scripts", "--no-audit", "--no-fund",
        "--registry=https://registry.npmjs.org", "--cache", state / "npm-cache",
        "--userconfig", user_config, "--globalconfig", global_config,
    ], env)


def install(prefix: Path, packages: list[str | Path], state: Path, env: dict[str, str]) -> Path:
    npm(["install", "--global", "--prefix", prefix, "--include=optional", *packages], state, env)
    return prefix / "bin/eas-mail-mcp"


def native(launcher: Path, version: str, prefix: Path, env: dict[str, str]) -> Path:
    assert command([launcher, "--version"], env).strip() == f"eas-mail-mcp {version}"
    binary = Path(command([launcher, "native-path"], env).strip()).resolve()
    assert binary.is_relative_to(prefix.resolve())
    assert binary.name == "eas-mail-mcp" and "eas-mail-mcp-darwin-arm64" in str(binary)
    return binary


def support_paths(env: dict[str, str]) -> tuple[Path, Path, Path]:
    support = Path(env["HOME"]) / "Library/Application Support/EAS Mail MCP"
    support.mkdir(parents=True, exist_ok=True)
    support.chmod(0o700)
    return support / "config.toml", support / "profiles.toml", support / "operations.sqlite"


def fixture_files(env: dict[str, str]) -> tuple[Path, Path, Path]:
    config, profiles, journal = support_paths(env)
    config.write_text(frozen("CONFIG"))
    profiles.write_text(frozen("PROFILES"))
    for path in (config, profiles):
        path.chmod(0o600)
    with sqlite3.connect(journal) as connection:
        connection.executescript(frozen("SCHEMA"))
        connection.execute(
            "INSERT INTO operations VALUES (?, 'work', 'mail_send', ?, ?, 'unknown', 0, 1788480000, 1788480001)",
            (frozen("UUID"), frozen("HMAC"), frozen("UUID")),
        )
    return config, profiles, journal


def legacy_row(journal: Path) -> tuple:
    with sqlite3.connect(journal) as connection:
        return connection.execute(
            "SELECT operation_id,account_id,kind,payload_hmac,client_id,status,completed_steps,created_at,updated_at FROM operations"
        ).fetchone()


def clean_install(packages: list[Path], state: Path, version: str) -> dict:
    location = state / "clean"
    env = child_environment(location)
    prefix = location / "prefix"
    launcher = install(prefix, packages, state, env)
    binary = native(launcher, version, prefix, env)
    local_version = json.loads(command([binary, "--version", "--verbose"], env))
    assert local_version["profile_store"]["configured"] is False
    help_text = command([binary, "mail", "--help"], env)
    assert all(name in help_text for name in ("auto-reply", "get-many", "thread", "set-flag", "batch"))
    report = location / "doctor.json"
    command([binary, "doctor", "--check", "--report", report], env, expected=1)
    diagnostic = json.loads(report.read_text())
    assert diagnostic["healthy"] is False
    assert not support_paths(env)[1].exists()
    return {"version": version, "native_sha256": digest(binary), "missing_setup_detected": True}


def upgrade(packages: list[Path], state: Path, version: str, previous: str) -> tuple[dict, Path, Path, dict]:
    location = state / "upgrade"
    env = child_environment(location)
    prefix = location / "prefix"
    old = install(prefix, [f"eas-mail-mcp-darwin-arm64@{previous}", f"eas-mail-mcp@{previous}"], state, env)
    previous_digest = digest(native(old, previous, prefix, env))
    config, profiles, journal = fixture_files(env)
    command([old, "profile", "validate"], env)
    before = (config.read_bytes(), profiles.read_bytes(), legacy_row(journal))
    launcher = install(prefix, packages, state, env)
    binary = native(launcher, version, prefix, env)
    command([binary, "profile", "validate"], env)
    inspected = json.loads(command([binary, "operation", "get", frozen("UUID")], env))
    assert inspected["error"] is None and inspected["data"]["status"] == "unknown"
    listed = json.loads(command([binary, "operation", "list", "--status", "unknown"], env))
    assert len(listed["data"]["operations"]) == 1
    assert before == (config.read_bytes(), profiles.read_bytes(), legacy_row(journal))
    with sqlite3.connect(journal) as connection:
        assert connection.execute("PRAGMA user_version").fetchone()[0] == 1
    return {
        "previous_version": previous, "previous_native_sha256": previous_digest,
        "version": version, "native_sha256": digest(binary),
        "config_profiles_unchanged": True, "legacy_row_preserved": True,
        "journal_schema": 1, "native_credentials_migration": "not tested; covered separately by MemorySecretStore fixtures",
    }, binary, prefix, env


async def mcp_smoke(binary: Path, state: Path, expected_tools: set[str]) -> dict:
    # macOS Keychain is keyed by the OS session, not HOME. This checks existence only;
    # no password, DeviceId, HMAC, or account credential is printed or exported.
    exists = subprocess.run(
        ["security", "find-generic-password", "-s", "eas-mail-mcp", "-a", "secrets-v1"],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=15,
    ).returncode == 0
    if not exists:
        return {"passed": False, "reason": "existing credential item absent; no creation attempted"}
    env = child_environment(state / "mcp")
    config, profiles, _ = fixture_files(env)
    config.write_text("version = 1\n")
    assert profiles.read_text() == frozen("PROFILES")
    process = await asyncio.create_subprocess_exec(
        str(binary), "serve", env=env, cwd=ROOT,
        stdin=asyncio.subprocess.PIPE, stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.PIPE, limit=2 * 1024 * 1024,
    )
    async def send(value: dict) -> None:
        process.stdin.write((json.dumps(value) + "\n").encode())
        await process.stdin.drain()
    async def rpc(identifier: int, method: str, params: dict) -> dict:
        await send({"jsonrpc": "2.0", "id": identifier, "method": method, "params": params})
        while True:
            line = await asyncio.wait_for(process.stdout.readline(), timeout=30)
            if not line:
                diagnostics = await asyncio.wait_for(process.stderr.read(4096), timeout=5)
                await process.wait()
                failure = {"passed": False, "reason": "packaged MCP ended before its response",
                           "exit_code": process.returncode}
                try:
                    code = json.loads(diagnostics).get("code")
                    if code in {"AUTH_REQUIRED", "CONFIG_INVALID", "STORAGE_ERROR"}:
                        failure["error_code"] = code
                except (ValueError, AttributeError):
                    pass
                raise McpCheckFailed(failure)
            response = json.loads(line)
            if response.get("id") == identifier:
                assert "error" not in response
                return response["result"]
    try:
        initialized = await rpc(1, "initialize", {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "isolated-package-acceptance", "version": "1.0"},
        })
        await send({"jsonrpc": "2.0", "method": "notifications/initialized"})
        tools = await rpc(2, "tools/list", {})
        names = {tool["name"] for tool in tools["tools"]}
        assert names == expected_tools
        accounts = await rpc(3, "tools/call", {"name": "accounts_list", "arguments": {}})
        assert accounts["structuredContent"]["data"]["accounts"] == []
        operation = await rpc(4, "tools/call", {
            "name": "operation_get", "arguments": {"operation_id": frozen("UUID")},
        })
        assert operation["structuredContent"]["data"]["status"] == "unknown"
        operations = await rpc(5, "tools/call", {"name": "operations_list", "arguments": {}})
        assert len(operations["structuredContent"]["data"]["operations"]) == 1
        result = {"passed": True, "tools": len(names), "accounts": 0,
                  "protocol_version": initialized["protocolVersion"],
                  "existing_keychain_read_only": True, "mailbox_network_requests": 0}
    finally:
        process.stdin.close()
        try:
            await asyncio.wait_for(process.wait(), timeout=10)
        except TimeoutError:
            process.kill()
            await process.wait()
            raise RuntimeError("packaged MCP did not exit after stdin closed")
    assert process.returncode == 0
    result["stdio_process_exited"] = True
    return result


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--packages-dir", type=Path, default=ROOT / "dist/npm")
    parser.add_argument("--previous-version", default="0.5.1")
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--existing-keychain-read", action="store_true")
    args = parser.parse_args()
    assert platform.system() == "Darwin" and platform.machine() == "arm64"
    version = json.loads((ROOT / "npm/eas-mail-mcp/package.json").read_text())["version"]
    packages = [(args.packages_dir / f"{name}-{version}.tgz").resolve()
                for name in ("eas-mail-mcp-darwin-arm64", "eas-mail-mcp")]
    assert all(path.is_file() for path in packages)
    report = {"kind": "local_development_package_acceptance", "published": False,
              "archives": {path.name: digest(path) for path in packages},
              "packaged_native_sha256": packaged_digest(packages[0])}
    try:
        with tempfile.TemporaryDirectory(prefix="eas-package-acceptance-") as temporary:
            state = Path(temporary)
            report["clean_install"] = clean_install(packages, state, version)
            assert report["clean_install"]["native_sha256"] == report["packaged_native_sha256"]
            checked, binary, prefix, env = upgrade(packages, state, version, args.previous_version)
            report["upgrade"] = checked
            assert checked["native_sha256"] == report["packaged_native_sha256"]
            if args.existing_keychain_read:
                contract = json.loads((ROOT / "contracts/v1.0.json").read_text())
                expected = set(contract["mcp"])
                try:
                    report["mcp"] = asyncio.run(mcp_smoke(binary, state, expected))
                except McpCheckFailed as error:
                    report["mcp"] = error.report
                except (AssertionError, RuntimeError, TimeoutError, ConnectionError):
                    report["mcp"] = {"passed": False, "reason": "packaged MCP protocol or lifecycle check failed"}
            else:
                report["mcp"] = {"passed": False, "reason": "existing Keychain read not requested"}
            config, profiles, journal = support_paths(env)
            before = (config.read_bytes(), profiles.read_bytes(), legacy_row(journal))
            npm(["uninstall", "--global", "--prefix", prefix, "eas-mail-mcp", "eas-mail-mcp-darwin-arm64"], state, env)
            assert not (prefix / "bin/eas-mail-mcp").exists()
            assert before == (config.read_bytes(), profiles.read_bytes(), legacy_row(journal))
            report["package_uninstall_preserves_local_data"] = True
            report["package_checks_passed"] = True
            report["requested_checks_passed"] = not args.existing_keychain_read or report["mcp"]["passed"]
    finally:
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))
    if not report["requested_checks_passed"]:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
