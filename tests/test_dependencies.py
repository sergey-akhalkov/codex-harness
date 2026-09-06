"""Safety and failure-path tests for explicit shared dependency maintenance."""
import base64
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tarfile
import tempfile
import sys
import time
import unittest
from unittest import mock

MODULE = Path(__file__).resolve().parents[1] / "tools/code-tools/dependencies.py"
SPEC = importlib.util.spec_from_file_location("harness_dependencies", MODULE)
dependencies = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(dependencies)


class DependencyTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="harness-dependencies-")
        self.root = Path(self.temp.name)

    def tearDown(self):
        self.temp.cleanup()

    def test_metadata_failure_is_not_current(self):
        def failed(_url):
            raise OSError("network unavailable")
        result = dependencies.check_release({"metadata": "https://pypi.org/pypi/example/json"}, failed)
        self.assertEqual(result["state"], "failed")
        self.assertIsNone(result["version"])

    def test_mcp_early_exit_reports_original_cause_without_waiting_or_secret(self):
        code = "import sys; sys.stdin.readline(); print('exact-build admission failed access_token=fixture-secret',file=sys.stderr); sys.exit(2)"
        began = time.monotonic()
        with self.assertRaises(RuntimeError) as caught:
            dependencies.mcp_probe([sys.executable, "-c", code], self.root / "probe", timeout=20)
        self.assertIn("exact-build admission failed", str(caught.exception))
        self.assertNotIn("fixture-secret", str(caught.exception))
        self.assertLess(time.monotonic() - began, 5)

    def test_prerelease_is_not_selected(self):
        result = dependencies.check_release({"metadata": "https://api.nuget.org/v3-flatcontainer/example/index.json"}, lambda _url: {"versions": ["1.0.0", "2.0.0-preview.1", "1.1.0"]})
        self.assertEqual(result["version"], "1.1.0")

    def test_github_prerelease_is_rejected(self):
        result = dependencies.check_release({"metadata": "https://api.github.com/repos/a/b/releases/latest"}, lambda _url: {"tag_name": "2.0.0", "prerelease": True})
        self.assertEqual(result["state"], "failed")

    def test_numeric_nuget_prerelease_is_not_stable(self):
        self.assertFalse(dependencies.stable_version("5.12.0-1.26426.8"))
        self.assertTrue(dependencies.stable_version("5.11.0"))
        self.assertTrue(dependencies.stable_version("2026-02-08"))

    def test_rust_placeholder_metadata_compares_declared_toolchain_cohort(self):
        result = dependencies.check_release({"metadata": "https://static.rust-lang.org/dist/channel-rust-stable.toml", "metadata_format": "rust-channel"},
            lambda _url: {"date": "2026-09-03", "pkg": {"rust": {"version": "1.98.1 (fixture)"}, "rust-analyzer-preview": {"version": "0.0.0"}}})
        self.assertEqual(result["version"], "1.98.1")
        self.assertEqual(result["version_kind"], "rust-toolchain-cohort")
        self.assertEqual(result["component_metadata_version"], "0.0.0")

    def test_lock_prevents_conflicting_update_and_releases(self):
        with dependencies.installation_lock(self.root, self.root / "shared"):
            with self.assertRaisesRegex(RuntimeError, "Another updater"):
                with dependencies.installation_lock(self.root, self.root / "shared"):
                    self.fail("Conflicting updater entered")
        with dependencies.installation_lock(self.root, self.root / "shared"):
            pass

    def test_integrity_mismatch_rejected(self):
        digest = base64.b64encode(hashlib.sha512(b"original").digest()).decode()
        with self.assertRaisesRegex(ValueError, "integrity mismatch"):
            dependencies.verify_integrity(b"modified", "sha512-" + digest)

    def test_archive_path_escape_rejected_before_extraction(self):
        buffer = io.BytesIO()
        with tarfile.open(fileobj=buffer, mode="w:gz") as archive:
            member = tarfile.TarInfo("../escape.txt")
            member.size = 4
            archive.addfile(member, io.BytesIO(b"oops"))
        with self.assertRaisesRegex(ValueError, "unsafe path"):
            dependencies.safe_extract_tar(buffer.getvalue(), self.root / "stage")
        self.assertFalse((self.root / "escape.txt").exists())

    def test_archive_symlink_rejected(self):
        buffer = io.BytesIO()
        with tarfile.open(fileobj=buffer, mode="w:gz") as archive:
            member = tarfile.TarInfo("package/link")
            member.type = tarfile.SYMTYPE
            member.linkname = "../../unrelated"
            archive.addfile(member)
        with self.assertRaisesRegex(ValueError, "unsafe path"):
            dependencies.safe_extract_tar(buffer.getvalue(), self.root / "stage")

    def installations(self):
        old, new, backup = [self.root / name for name in ("installed", "candidate", "rollback")]
        old.mkdir()
        new.mkdir()
        (old / "tool").write_text("old local changes")
        (new / "tool").write_text("new")
        return old, new, backup

    def test_failed_validation_restores_exact_previous_installation(self):
        old, new, backup = self.installations()
        def fail(_path):
            raise RuntimeError("real consumer regression")
        with self.assertRaisesRegex(RuntimeError, "consumer regression"):
            dependencies.atomic_promote(new, old, backup, fail)
        self.assertEqual((old / "tool").read_text(), "old local changes")
        self.assertEqual((new / "tool").read_text(), "new")
        self.assertFalse(backup.exists())

    def test_successful_update_retains_exact_rollback(self):
        old, new, backup = self.installations()
        result = dependencies.atomic_promote(new, old, backup, lambda path: (path / "tool").read_text())
        self.assertEqual(result, "new")
        self.assertEqual((backup / "tool").read_text(), "old local changes")

    def auxiliary(self):
        target, before, after = [self.root / name for name in ("protected.json", "before.json", "after.json")]
        target.write_text('{"credential":"fixture-private", "version":"old"}')
        before.write_bytes(target.read_bytes())
        after.write_text('{"credential":"fixture-private", "version":"new"}')
        return {"path": target, "before_path": before, "after_path": after}

    def test_auxiliary_promotion_and_scoped_recovery_do_not_store_secret(self):
        old, new, backup = self.installations()
        aux = self.auxiliary()
        journal = self.root / "transaction.json"
        def verify(_path):
            self.assertEqual(json.loads(aux["path"].read_text())["version"], "new")
            return True
        dependencies.atomic_promote(new, old, backup, verify, journal, [aux])
        self.assertNotIn("fixture-private", journal.read_text())
        self.assertFalse(aux["after_path"].exists())
        self.assertEqual(dependencies.recover_transaction(journal, True)["state"], "restored")
        self.assertEqual(aux["path"].read_bytes(), aux["before_path"].read_bytes())
        self.assertEqual((old / "tool").read_text(), "old local changes")

    def test_auxiliary_unknown_change_preserves_both_halves(self):
        old, new, backup = self.installations()
        aux = self.auxiliary()
        journal = self.root / "transaction.json"
        dependencies.atomic_promote(new, old, backup, lambda _path: True, journal, [aux])
        aux["path"].write_text("operator changed after commit")
        result = dependencies.recover_transaction(journal, True)
        self.assertEqual(result["state"], "pending")
        self.assertEqual(aux["path"].read_text(), "operator changed after commit")
        self.assertEqual((old / "tool").read_text(), "new")
        self.assertEqual((backup / "tool").read_text(), "old local changes")

    def test_auxiliary_failure_restores_both_halves(self):
        old, new, backup = self.installations()
        aux = self.auxiliary()
        def fail(_path):
            raise RuntimeError("consumer regression")
        with self.assertRaisesRegex(RuntimeError, "consumer regression"):
            dependencies.atomic_promote(new, old, backup, fail, auxiliary_files=[aux])
        self.assertEqual(aux["path"].read_bytes(), aux["before_path"].read_bytes())
        self.assertEqual((old / "tool").read_text(), "old local changes")

    def test_crash_after_first_move_recovers_from_prepared_journal(self):
        old, new, backup = self.installations()
        journal = self.root / "transaction.json"
        data = {"schema_version": 1, "owner": "codex-harness-dependencies", "kind": "replace-directory", "phase": "prepared",
                "installation": str(old), "candidate": str(new), "backup": str(backup),
                "prior_identity": dependencies.tree_identity(old), "candidate_identity": dependencies.tree_identity(new)}
        dependencies.atomic_json(journal, data)
        old.rename(backup)  # simulate a process killed before updating the persisted phase
        result = dependencies.recover_transaction(journal)
        self.assertEqual(result["state"], "restored")
        self.assertEqual((old / "tool").read_text(), "old local changes")
        self.assertEqual((new / "tool").read_text(), "new")

    def test_committed_transaction_can_be_scoped_rollback_for_later_activation_failure(self):
        old, new, backup = self.installations()
        journal = self.root / "transaction.json"
        dependencies.atomic_promote(new, old, backup, lambda _path: True, journal)
        self.assertEqual(dependencies.recover_transaction(journal)["state"], "committed")
        self.assertEqual(dependencies.recover_transaction(journal, rollback_committed=True)["state"], "restored")
        self.assertEqual((old / "tool").read_text(), "old local changes")

    def test_recovery_preserves_changes_made_after_update(self):
        old, new, backup = self.installations()
        journal = self.root / "transaction.json"
        dependencies.atomic_promote(new, old, backup, lambda _path: True, journal)
        (old / "tool").write_text("new user change")
        result = dependencies.recover_transaction(journal, rollback_committed=True)
        self.assertEqual(result["state"], "pending")
        self.assertEqual((old / "tool").read_text(), "new user change")
        self.assertEqual((backup / "tool").read_text(), "old local changes")

    def test_new_install_crash_after_move_restores_absence(self):
        candidate = self.root / "staging/candidate"
        candidate.mkdir(parents=True)
        (candidate / "tool").write_text("new")
        installation = self.root / "shared"
        persist = dependencies.atomic_json
        def crash_on_commit(path, value):
            if value["phase"] == "committed":
                raise SystemExit("simulated process loss")
            persist(path, value)
        with mock.patch.object(dependencies, "atomic_json", side_effect=crash_on_commit):
            with self.assertRaises(SystemExit):
                dependencies.activate_new_directory(candidate, installation, self.root, "fixture")
        self.assertTrue(installation.exists())
        journal = next((self.root / "transactions").glob("**/*.json"))
        self.assertEqual(json.loads(journal.read_text())["phase"], "prepared")
        result = dependencies.recover_transaction(journal)
        self.assertEqual(result["state"], "restored")
        self.assertFalse(installation.exists())
        self.assertTrue(Path(result["retained_candidate"]).is_dir())

    def test_new_install_journal_precedes_first_move(self):
        candidate = self.root / "staging/candidate"
        candidate.mkdir(parents=True)
        (candidate / "tool").write_text("new")
        installation = self.root / "shared"
        rename = dependencies.rename_checked
        def observe(source, destination):
            journals = list((self.root / "transactions").glob("**/*.json"))
            self.assertEqual(len(journals), 1)
            self.assertEqual(json.loads(journals[0].read_text())["phase"], "prepared")
            rename(source, destination)
        with mock.patch.object(dependencies, "rename_checked", side_effect=observe):
            dependencies.activate_new_directory(candidate, installation, self.root, "fixture")

    def test_new_file_crash_after_link_recovers_prior_absence(self):
        candidate = self.root / "staging/dictionary.txt"
        candidate.parent.mkdir()
        candidate.write_text("owned public dictionary")
        target = self.root / "models/dictionary.txt"
        target.parent.mkdir()
        persist = dependencies.atomic_json
        def crash_on_commit(path, value):
            if value["phase"] == "committed":
                raise SystemExit("simulated process loss")
            persist(path, value)
        with mock.patch.object(dependencies, "atomic_json", side_effect=crash_on_commit):
            with self.assertRaises(SystemExit):
                dependencies.activate_new_file(candidate, target, self.root, "dictionary")
        journal = next((self.root / "transactions").glob("**/*.json"))
        result = dependencies.recover_transaction(journal)
        self.assertEqual(result["state"], "restored")
        self.assertFalse(target.exists())
        self.assertEqual(Path(result["retained_file"]).read_text(), "owned public dictionary")

    def test_new_file_does_not_overwrite_concurrent_creation(self):
        candidate = self.root / "staging/dictionary.txt"
        candidate.parent.mkdir()
        candidate.write_text("candidate")
        target = self.root / "dictionary.txt"
        link = dependencies.os.link
        def race(source, destination):
            Path(destination).write_text("unrelated actor")
            link(source, destination)
        with mock.patch.object(dependencies.os, "link", side_effect=race):
            with self.assertRaises(FileExistsError):
                dependencies.activate_new_file(candidate, target, self.root, "dictionary")
        self.assertEqual(target.read_text(), "unrelated actor")

    def test_corrupt_journal_preserves_files_and_does_not_block_recoverable_peer(self):
        state, home = self.root / "state", self.root / "user"
        candidate = state / "staging/fixture"
        candidate.mkdir(parents=True)
        (candidate / "tool").write_text("owned new backend")
        installation = home / ".serena/language_servers/static/Fixture/backend"
        installation.parent.mkdir(parents=True)
        with mock.patch.object(dependencies, "TRANSACTION_ID", "fixture"):
            good = dependencies.activate_new_directory(candidate, installation, state, "fixture")
        bad = state / "transactions/fixture/corrupt.json"
        malformed = {"owner": "codex-harness-dependencies", "kind": "replace-directory", "phase": "prior-moved"}
        bad.write_text(json.dumps(malformed))
        before = bad.read_bytes()
        def no_consumers(_reader, records):
            for record in records:
                record["active_consumers"] = {"state": "observed", "processes": []}
        with mock.patch.object(dependencies.discovery.Discovery, "inspect_consumers", no_consumers):
            result = dependencies.recover_dependencies(state, home, "fixture", rollback_committed=True)
        self.assertFalse(result["complete"])
        self.assertEqual({item["state"] for item in result["results"]}, {"pending", "restored"})
        self.assertFalse(installation.exists())
        self.assertEqual(bad.read_bytes(), before)
        self.assertEqual(json.loads(Path(good).read_text())["phase"], "restored")

    def test_missing_prior_hash_cannot_claim_missing_installation_restored(self):
        journal = self.root / "bad.json"
        journal.write_text(json.dumps({"owner": "codex-harness-dependencies", "kind": "replace-directory", "phase": "prior-moved",
            "installation": str(self.root / "missing"), "candidate": str(self.root / "candidate"), "backup": str(self.root / "backup"),
            "candidate_identity": "0" * 64, "prior_identity": None}))
        result = dependencies.recover_transaction(journal)
        self.assertEqual(result["state"], "pending")
        self.assertIn("prior_identity", result["reason"])

    def test_existing_ocr_models_are_reused_without_fetch_or_replacement(self):
        model_root = self.root / "user/AppData/Roaming/Nuphus/models"
        model_root.mkdir(parents=True)
        before = {}
        for name in dependencies.NUPHUS_MODELS:
            target = model_root / name
            target.write_bytes(b"existing alternate model".ljust(1_000_000, b"x"))
            before[name] = target.read_bytes()
        (model_root / "ch_PP-OCR_keys_v1.txt").write_text("dictionary\n" * 2000)
        with mock.patch.object(dependencies, "fetch", side_effect=AssertionError("reuse must be offline")):
            result = dependencies.provision_nuphus_models(self.root / "user", self.root / "state")
        self.assertEqual(result["state"], "reused")
        for name, data in before.items():
            self.assertEqual((model_root / name).read_bytes(), data)
        self.assertIn("native OCR validation required", result["models"][0]["compatibility"])

    def test_mismatched_ocr_download_cannot_become_runtime_cache(self):
        with mock.patch.object(dependencies, "fetch", return_value=b"unproved".ljust(1_000_000, b"x")):
            with self.assertRaisesRegex(ValueError, "compatibility fingerprint"):
                dependencies.provision_nuphus_models(self.root / "user", self.root / "state")
        self.assertFalse((self.root / "user/AppData/Roaming/Nuphus/models").exists())
        self.assertEqual(list((self.root / "state").glob("transactions/**/*.json")), [])

    def test_explicit_ocr_download_has_durable_inverse(self):
        raw = b"proved fixture bytes".ljust(1_000_000, b"x")
        name = next(iter(dependencies.NUPHUS_MODELS))
        fixture = {name: {"url": "https://huggingface.co/official/fixture", "sha256": hashlib.sha256(raw).hexdigest()}}
        model_root = self.root / "user/AppData/Roaming/Nuphus/models"
        model_root.mkdir(parents=True)
        (model_root / "ch_PP-OCR_keys_v1.txt").write_text("dictionary\n" * 2000)
        with mock.patch.dict(dependencies.NUPHUS_MODELS, fixture, clear=True), mock.patch.object(dependencies, "fetch", return_value=raw):
            result = dependencies.provision_nuphus_models(self.root / "user", self.root / "state")
        self.assertEqual(result["state"], "installed-unverified")
        self.assertEqual((model_root / name).read_bytes(), raw)
        recovered = dependencies.recover_transaction(result["models"][0]["transaction_journal"], rollback_committed=True)
        self.assertEqual(recovered["state"], "restored")
        self.assertFalse((model_root / name).exists())
        self.assertEqual(Path(recovered["retained_file"]).read_bytes(), raw)

    def test_tree_identity_preserves_existing_platform_path_order(self):
        for name in ("z.txt", "Abc.txt", "abc-more.txt", "Sub/other.txt"):
            file = self.root / name
            file.parent.mkdir(exist_ok=True)
            file.write_text(name)
        expected = hashlib.sha256()
        for file in sorted(self.root.rglob("*")):
            if file.is_file():
                expected.update(file.relative_to(self.root).as_posix().encode() + b"\0")
                expected.update(bytes.fromhex(dependencies.discovery.fingerprint(file)))
        self.assertEqual(dependencies.tree_identity(self.root), expected.hexdigest())

    def test_nested_staging_rejected_before_changing_installation(self):
        old, _new, backup = self.installations()
        candidate = old / "staging"
        candidate.mkdir()
        with self.assertRaisesRegex(ValueError, "outside the active"):
            dependencies.atomic_promote(candidate, old, backup, lambda _path: None)
        self.assertEqual((old / "tool").read_text(), "old local changes")

    def test_plan_does_not_install_missing_conditional_backends(self):
        def spec(id, required):
            return {"id": id, "package": id, "required": required, "runtime": [], "metadata": "https://registry.npmjs.org/" + id + "/latest"}
        def record(id):
            return {"id": id, "status": "missing", "version": None, "installation_root": None, "active_consumers": {"state": "observed", "processes": []}}
        requested = []
        def metadata(url):
            requested.append(url)
            return {"version": "1.0.0"}
        result = dependencies.plan({"mcp": [], "languages": [spec("python", True), spec("yaml", False)]}, {"mcp": [], "languages": [record("python"), record("yaml")]}, metadata)
        self.assertEqual([x["action"] for x in result["items"]], ["install-required", "conditional-absent"])
        self.assertEqual(len(requested), 1)
        self.assertTrue(result["read_only"])
        self.assertEqual(list(self.root.iterdir()), [])

    def test_newer_installation_is_not_downgraded(self):
        spec = {"id": "tool", "package": "tool", "runtime": [], "metadata": "https://registry.npmjs.org/tool/latest"}
        record = {"id": "tool", "status": "adopted", "version": "3.0.0", "installation_root": "existing", "active_consumers": {"state": "observed", "processes": []}}
        result = dependencies.plan({"mcp": [spec], "languages": []}, {"mcp": [record], "languages": []}, lambda _url: {"version": "2.0.0"})
        self.assertEqual(result["items"][0]["action"], "reuse")


if __name__ == "__main__":
    unittest.main()
