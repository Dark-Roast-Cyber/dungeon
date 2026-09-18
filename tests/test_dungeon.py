"""Offline contracts; these tests do not establish runtime isolation."""

from contextlib import redirect_stdout, redirect_stderr
import copy
import io
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from dungeon.cli import main, parser
from dungeon.config import Config, DungeonError, LABEL, decode, digest, load_config, starter_policy, write_new
from dungeon.runtime import Runtime, create_arguments, host_environment


class ProjectFixture:
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.project = self.root / "project with spaces"
        self.project.mkdir()
        self.directory = self.root / "config"
        with redirect_stdout(io.StringIO()):
            result = main([
                "init", "--non-interactive", "--directory", str(self.directory),
                "--project", str(self.project), "--name", "demo", "--gateway", "local",
            ])
        self.assertEqual(result, 0)
        self.path = self.directory / "dungeon.json"
        self.config = load_config(self.path)

    def replace_config(self, key, value):
        data = json.loads(self.path.read_text())
        data[key] = value
        self.path.write_text(json.dumps(data))


class ConfigTests(ProjectFixture, unittest.TestCase):
    def test_init_does_not_start_process(self):
        with patch("subprocess.run") as run, redirect_stdout(io.StringIO()):
            self.assertEqual(main(["--config", str(self.path), "plan"]), 0)
        run.assert_not_called()

    def test_plan_is_json_and_preserves_arguments(self):
        output = io.StringIO()
        with redirect_stdout(output):
            self.assertEqual(main(["--config", str(self.path), "plan"]), 0)
        plan = json.loads(output.getvalue())
        self.assertIn("--no-auto-providers", plan["command"])
        self.assertNotIn("--upload", plan["command"])
        self.assertNotIn("--env", plan["command"])
        self.assertEqual(plan["fingerprint"], self.config.fingerprint)

    def test_unknown_config_rejected(self):
        self.replace_config("privileged", True)
        with self.assertRaises(DungeonError):
            load_config(self.path)

    def test_bool_not_schema_version(self):
        self.replace_config("schema", True)
        with self.assertRaises(DungeonError):
            load_config(self.path)

    def test_invalid_inputs(self):
        original = self.path.read_text()
        for key, value in [
            ("name", "--privileged"), ("gateway", "../host"), ("providers", ["TOKEN=secret"]),
            ("providers", ["same", "same"]), ("providers", "x"), ("agent", "chatgpt"),
            ("project", "."), ("project", str(Path.home())), ("project", self.project.anchor),
            ("image", "./Dockerfile"), ("image", "base"), ("image", "image;echo hi"),
            ("policy", "../../policy.json"),
        ]:
            with self.subTest(key=key, value=value):
                self.path.write_text(original)
                self.replace_config(key, value)
                with self.assertRaises(DungeonError):
                    load_config(self.path)

    def test_config_inside_project_rejected(self):
        nested = self.project / "config"
        nested.mkdir()
        (nested / "dungeon.json").write_text(self.path.read_text())
        (nested / "policy.json").write_text((self.directory / "policy.json").read_text())
        with self.assertRaises(DungeonError):
            load_config(nested / "dungeon.json")

    def test_deleted_host_project_preserves_config_identity(self):
        old = self.config.fingerprint
        self.project.rmdir()
        self.assertEqual(load_config(self.path).fingerprint, old)

    def test_duplicate_json_keys_and_nonfinite_rejected(self):
        for value in ('{"schema":1,"schema":2}', '{"score":NaN}', '[] bad'):
            with self.subTest(value=value), self.assertRaises(DungeonError):
                decode(value)

    def test_existing_file_never_overwritten(self):
        old = self.path.read_bytes()
        with self.assertRaises(FileExistsError):
            write_new(self.path, {})
        self.assertEqual(old, self.path.read_bytes())

    def test_fingerprint_tracks_policy_not_json_format(self):
        old = self.config.fingerprint
        data = json.loads(self.path.read_text())
        self.path.write_text(json.dumps(data, sort_keys=True))
        self.assertEqual(old, load_config(self.path).fingerprint)
        policy_path = self.directory / "policy.json"
        policy = json.loads(policy_path.read_text())
        policy["network_policies"]["extra"] = {"endpoints": [{"host": "example.com", "port": 443}]}
        policy_path.write_text(json.dumps(policy))
        self.assertNotEqual(old, load_config(self.path).fingerprint)
        self.assertLessEqual(len(old), 63)

    def test_unsafe_policy_modes_rejected(self):
        path = self.directory / "policy.json"
        original = json.loads(path.read_text())
        policy = copy.deepcopy(original)
        policy["landlock"]["compatibility"] = "best_effort"
        path.write_text(json.dumps(policy))
        with self.assertRaises(DungeonError):
            load_config(self.path)
        policy = copy.deepcopy(original)
        policy["network_policies"]["models"]["endpoints"] = [
            {"host": "example.com", "port": 443, "tls": "terminate", "enforcement": "audit"}
        ]
        path.write_text(json.dumps(policy))
        with self.assertRaises(DungeonError):
            load_config(self.path)

    def test_minimal_profile_omits_development_endpoints(self):
        self.assertEqual(set(starter_policy("minimal")["network_policies"]), {"models"})
        self.assertIn("development", starter_policy("development")["network_policies"])

    def test_explicit_config_and_upload_consent_required(self):
        with patch("dungeon.cli.Runtime") as runtime, redirect_stderr(io.StringIO()):
            self.assertEqual(main(["status"]), 1)
            self.assertEqual(main(["--config", str(self.path), "upload"]), 1)
        runtime.assert_not_called()

    def test_run_parser_preserves_agent_flags(self):
        args = parser().parse_args(["run", "opencode", "--continue"])
        self.assertEqual(args.arguments, ["--continue"])
        args = parser().parse_args(["exec", "--", "printf", "a; echo unsafe"])
        self.assertEqual(args.arguments, ["--", "printf", "a; echo unsafe"])

    def test_host_environment_is_allowlisted(self):
        environment = {
            "PATH": str(self.project) + os.pathsep + "." + os.pathsep + str(self.root / "trusted-bin"),
            "HOME": str(self.root), "OPENAI_API_KEY": "secret", "OPENSHELL_GATEWAY_INSECURE": "true",
            "OPENSHELL_SANDBOX_POLICY": "evil.yaml", "LD_PRELOAD": "evil.so", "BASH_ENV": "evil.sh",
        }
        with patch.dict(os.environ, environment, clear=True):
            env = host_environment(self.project)
        self.assertEqual(env["PATH"], str(self.root / "trusted-bin"))
        for forbidden in set(environment) - {"HOME", "PATH"}:
            self.assertNotIn(forbidden, env)


class RuntimeTests(ProjectFixture, unittest.TestCase):
    """Fake the external command boundary, not the identity checks."""

    def setUp(self):
        super().setUp()
        self.runtime = Runtime.__new__(Runtime)
        self.runtime.config = self.config
        self.runtime.binary = str(self.root / "bin" / "openshell")
        self.runtime.env = {"PATH": os.defpath}
        self.remote = {
            "name": "demo", "workspace": "default", "id": "sandbox-one", "phase": "Ready",
            "labels": {LABEL: self.config.fingerprint}, "policy_source": "sandbox", "policy": self.config.policy,
        }

    def bind(self):
        write_new(self.config.state_path, {
            "schema": 1, "id": "sandbox-one", "config": self.config.fingerprint,
            "observed_policy": digest(self.remote["policy"]),
        })

    def test_create_records_verified_identity(self):
        with patch.object(self.runtime, "call", return_value="{}") as call, patch.object(self.runtime, "get", return_value=self.remote), redirect_stdout(io.StringIO()):
            self.assertEqual(self.runtime.create(), 0)
        args = call.call_args.args[0]
        self.assertIn("--detach", args)
        self.assertIn("--no-auto-providers", args)
        self.assertEqual(json.loads(self.config.state_path.read_text())["id"], "sandbox-one")
        self.assertFalse(list(self.directory.glob("create-*")))

    def test_create_failure_does_not_record_or_delete(self):
        with patch.object(self.runtime, "call", side_effect=DungeonError("failed")) as call:
            with self.assertRaises(DungeonError):
                self.runtime.create()
        self.assertFalse(self.config.state_path.exists())
        self.assertEqual(call.call_count, 1)

    def test_existing_create_is_idempotent(self):
        self.bind()
        with patch.object(self.runtime, "get", return_value=self.remote), patch.object(self.runtime, "call") as call, redirect_stdout(io.StringIO()):
            self.assertEqual(self.runtime.create(), 0)
        call.assert_not_called()

    def test_execution_checks_identity_policy_and_readiness(self):
        self.bind()
        mutations = [
            ("id", "other"), ("labels", {}), ("phase", "Stopped"), ("name", "other"),
            ("workspace", "other"), ("policy", {}),
        ]
        for key, value in mutations:
            remote = dict(self.remote)
            remote[key] = value
            with self.subTest(key=key), patch.object(self.runtime, "get", return_value=remote), patch.object(self.runtime, "call") as call:
                with self.assertRaises(DungeonError):
                    self.runtime.execute(["opencode"])
                call.assert_not_called()

    def test_exec_is_argument_vector_and_propagates_failure(self):
        self.bind()
        with patch.object(self.runtime, "get", return_value=self.remote), patch.object(self.runtime, "call", return_value=7) as call:
            self.assertEqual(self.runtime.execute(["printf", "hello; touch /host"], tty=True), 7)
        self.assertEqual(call.call_args.args[0], [
            "sandbox", "exec", "--name", "demo", "--workdir", "/sandbox/project", "--tty",
            "--", "printf", "hello; touch /host",
        ])

    def test_stop_can_verify_identity_without_policy_approval(self):
        self.bind()
        self.remote["policy"] = {"changed": True}
        with patch.object(self.runtime, "get", return_value=self.remote):
            self.runtime.checked(ready=False, policy=False)

    def test_no_binding_is_not_implicit_adoption(self):
        with self.assertRaises(FileNotFoundError):
            self.runtime.checked()

    def test_subprocess_has_no_shell_or_inherited_secrets(self):
        result = subprocess.CompletedProcess([], 4)
        with patch("subprocess.run", return_value=result) as run:
            self.assertEqual(self.runtime.call(["sandbox", "stop", "demo"]), 4)
        self.assertFalse(run.call_args.kwargs["shell"])
        self.assertEqual(run.call_args.kwargs["cwd"], self.directory)
        self.assertEqual(run.call_args.args[0][1:5], ["--gateway", "local", "--workspace", "default"])
        self.assertEqual(run.call_args.kwargs["env"], self.runtime.env)

    def test_backend_error_does_not_repeat_raw_stderr(self):
        result = subprocess.CompletedProcess([], 1, stdout="", stderr="secret-value")
        with patch("subprocess.run", return_value=result), self.assertRaises(DungeonError) as error:
            self.runtime.call(["sandbox", "get", "demo"], capture=True)
        self.assertNotIn("secret-value", str(error.exception))

    def test_wrong_backend_version_rejected(self):
        with patch("shutil.which", return_value=str(self.root / "openshell")), patch.object(Runtime, "call", return_value="openshell 0.0.117"):
            with self.assertRaises(DungeonError):
                Runtime(self.config)

    def test_upload_is_explicit_and_scoped(self):
        self.bind()
        (self.project / "file.txt").write_text("safe fixture")
        with patch.object(self.runtime, "get", return_value=self.remote), patch.object(self.runtime, "call", return_value=0) as call:
            self.assertEqual(self.runtime.upload(), 0)
        self.assertEqual(call.call_args.args[0], ["sandbox", "upload", "demo", str(self.project), "/sandbox/project"])

    @unittest.skipIf(os.name == "nt", "Symlink creation requires Windows privileges")
    def test_upload_rejects_outside_symlink(self):
        self.bind()
        (self.project / "outside").symlink_to(self.root)
        with patch.object(self.runtime, "get", return_value=self.remote), patch.object(self.runtime, "call") as call:
            with self.assertRaises(DungeonError):
                self.runtime.upload()
        call.assert_not_called()

    def test_telltale_is_explicit_dry_run_inside_guest(self):
        with patch("dungeon.cli.Runtime") as runtime, redirect_stderr(io.StringIO()):
            runtime.return_value.execute.return_value = 0
            self.assertEqual(main(["--config", str(self.path), "telltale"]), 0)
        self.assertEqual(runtime.return_value.execute.call_args.args[0], [
            "telltale", "scan", "--once", "--dry-run", "--no-local-config", "--root", "/sandbox",
        ])


if __name__ == "__main__":
    unittest.main()
