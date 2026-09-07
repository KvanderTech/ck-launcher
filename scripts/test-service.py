"""Exercises the real service in isolated APPDATA; never downloads or executes a mod."""
import io, json, os, queue, subprocess, sys, tempfile, threading, zipfile
from pathlib import Path

def main():
    executable = Path(sys.argv[1]).resolve()
    with tempfile.TemporaryDirectory(prefix="ck-protocol-test-") as directory:
        env = dict(os.environ, APPDATA=directory)
        process = subprocess.Popen([str(executable)], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, encoding="utf-8", env=env)
        output = queue.Queue()
        threading.Thread(target=lambda: [output.put(line) for line in process.stdout], daemon=True).start()
        sequence = 0
        def call(method, params=None):
            nonlocal sequence
            sequence += 1
            process.stdin.write(json.dumps({"id":sequence,"method":method,"params":params or {}}, ensure_ascii=False)+"\n"); process.stdin.flush()
            while True:
                message = json.loads(output.get(timeout=30))
                if message.get("id") == sequence: return message
        try:
            assert call("hello")["result"]["protocolVersion"] == 1
            assert call("get_profile")["result"]["id"] == "default"
            assert call("list_builds")["result"] == []
            assert call("unknown")["error"]["code"] == "unknown_method"
            build = call("create_build", {"name":"Тест с пробелами", "gameVersion":"1.20.1", "loader":"vanilla"})["result"]
            assert len(call("list_builds")["result"]) == 1
            assert call("confirm_mrpack", {"sha256":"0"*64})["error"]["code"] == "preview_required"
            index={"formatVersion":1,"game":"minecraft","versionId":"test","name":"Harmless regression","dependencies":{"minecraft":"1.20.1"},"files":[{"path":"mods/test.jar","hashes":{},"downloads":["https://cdn.modrinth.com/test.jar"],"fileSize":1}]}
            archive=Path(directory)/"invalid.mrpack"
            with zipfile.ZipFile(archive,"w") as z: z.writestr("modrinth.index.json",json.dumps(index))
            rejected=call("preview_mrpack", {"sourcePath":str(archive)})
            assert rejected["error"]["code"] == "download_hash_required", rejected
            assert len(call("list_builds")["result"]) == 1
            assert not (Path(build["gameDir"])/"mods/test.jar").exists()
            assert call("open_build_path", {"buildId":build["id"],"relativePath":"../outside.exe"})["error"]["code"] == "invalid_path"
            assert call("delete_build", {"buildId":build["id"]})["result"] is None
            assert call("list_builds")["result"] == []
            # A harmless local modpack can be repaired without invented Modrinth IDs.
            index["files"] = []
            with zipfile.ZipFile(archive, "w") as z:
                z.writestr("modrinth.index.json", json.dumps(index))
                z.writestr("overrides/config/settings.txt", "initial")
            preview = call("preview_mrpack", {"sourcePath": str(archive)})["result"]
            pack = call("confirm_mrpack", {"sha256": preview["sha256"]})["result"]
            assert call("confirm_mrpack", {"sha256": preview["sha256"]})["error"]["code"] == "preview_required"
            new_build = call("list_builds")["result"][0]
            config = Path(new_build["gameDir"]) / "config/settings.txt"
            assert config.read_text() == "initial"
            config.write_text("user setting")
            repaired = call("repair_build", {"buildId": new_build["id"]})
            assert "result" in repaired, repaired
            assert config.read_text() == "user setting"
            config.unlink()
            assert "result" in call("repair_build", {"buildId": new_build["id"]})
            assert config.read_text() == "initial"
            # Deleting a disabled mod removes the disabled file and its database row together.
            import sqlite3
            moddir = Path(new_build["gameDir"]) / "mods"
            moddir.mkdir(exist_ok=True)
            disabled = moddir / "harmless.jar.disabled"
            disabled.write_bytes(b"not executable")
            with sqlite3.connect(Path(directory)/"CKLauncher/launcher.sqlite3") as db:
                db.execute("INSERT INTO installed_content (id,build_id,project_id,version_id,project_type,title,filename,icon_url,enabled,installed_at) VALUES (?,?,?,?,?,?,?,?,?,?)", ("test-disabled",new_build["id"],"disabled","local","mod","Disabled","harmless.jar",None,0,0))
            db.close()
            assert "result" in call("remove_installed_content", {"buildId":new_build["id"], "projectId":"disabled"})
            assert not disabled.exists()
            print("PASS: protocol, local startup, Unicode paths, build lifecycle, preview consent, harmless PoC rejection, traversal rejection, consent replay prevention, local pack repair, user config preservation, disabled mod deletion")
        finally:
            process.stdin.close()
            try: process.wait(timeout=5)
            except subprocess.TimeoutExpired: process.kill(); process.wait(timeout=5)
if __name__ == "__main__": main()
