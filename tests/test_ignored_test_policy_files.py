"""The ignored-test policy rows live in one file per topic, and a `needs` variable's tests run one per process."""
import contextlib
import io
import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from tools.ci import ignored_tests


class PolicyFiles(unittest.TestCase):
    def setUp(self):
        scratch = tempfile.TemporaryDirectory()
        self.addCleanup(scratch.cleanup)
        self.folder = Path(scratch.name) / "ignored-tests"
        self.folder.mkdir()
        self.retired = Path(scratch.name) / "ignored-tests-known-failures.txt"
        for name, value in (("POLICY", self.folder), ("RETIRED", self.retired)):
            patcher = patch.object(ignored_tests, name, value)
            patcher.start()
            self.addCleanup(patcher.stop)

    def write(self, name, text):
        (self.folder / name).write_text(text)

    def exit_message(self, call):
        with self.assertRaises(SystemExit) as stopped:
            call()
        return str(stopped.exception.code)

    def test_rows_come_from_every_topic_file_in_name_order(self):
        self.write("gpu.txt", "# adapters\npkg b::test adapter gpu\n")
        self.write("flaky.txt", "pkg a::test #1 flaky # a note\n\npkg c::test #2\n")
        self.write("README.md", "pkg ignored::row #3\n")
        self.assertEqual(ignored_tests.listed_entries(), [
            ignored_tests.Row("pkg", "a::test", "#1", "flaky", "tools/ci/ignored-tests/flaky.txt"),
            ignored_tests.Row("pkg", "c::test", "#2", None, "tools/ci/ignored-tests/flaky.txt"),
            ignored_tests.Row("pkg", "b::test", "adapter", "gpu", "tools/ci/ignored-tests/gpu.txt"),
        ])

    def test_a_manual_row_says_who_runs_it_and_why(self):
        self.write("manual-pkg.txt", "pkg a::bench by-hand manual # a microbenchmark\n"
                                      "pkg b::child subprocess manual # a child of b::parent\n"
                                      "pkg c::fixture wasm-mods.yml manual # built by the workflow\n")
        self.assertEqual([(row.test, row.reference, row.mark) for row in ignored_tests.listed_entries()],
                         [("a::bench", "by-hand", "manual"), ("b::child", "subprocess", "manual"),
                          ("c::fixture", "wasm-mods.yml", "manual")])
        self.assertEqual(ignored_tests.known_failures()[("pkg", "a::bench")], "manual")
        for row, refusal in (("pkg a::bench by-hand manual\n", "WHY"), ("pkg a::bench somebody manual # x\n", "WHO"),
                             ("pkg a::bench nothing.yml manual # x\n", "no workflow")):
            self.write("manual-pkg.txt", row)
            self.assertIn(refusal, self.exit_message(ignored_tests.listed_entries))

    def test_a_test_in_two_files_names_both(self):
        self.write("flaky.txt", "pkg a::test #1 flaky\n")
        self.write("issue-9.txt", "# tracked\npkg a::test #9\n")
        message = self.exit_message(ignored_tests.listed_entries)
        self.assertIn("tools/ci/ignored-tests/issue-9.txt:2", message)
        self.assertIn("tools/ci/ignored-tests/flaky.txt:1", message)

    def test_a_file_at_the_retired_path_names_the_folder(self):
        self.write("flaky.txt", "pkg a::test #1 flaky\n")
        self.retired.write_text("pkg b::test #2\n")
        self.assertIn("tools/ci/ignored-tests/<topic>.txt", self.exit_message(ignored_tests.listed_entries))

    def test_a_folder_without_rows_or_with_another_file_is_refused(self):
        self.write("README.md", "# format\n")
        self.write("flaky.txt", "# only a comment\n")
        self.assertIn("has no rows", self.exit_message(ignored_tests.listed_entries))
        self.write("flaky.txt", "pkg a::test #1 flaky\n")
        self.write("issue-3000.text", "pkg b::test #3000\n")
        self.assertIn("not read: issue-3000.text", self.exit_message(ignored_tests.listed_entries))

    def test_daily_rows_name_a_target_and_a_reason_and_are_not_known_failures(self):
        self.write("daily-pkg.txt", "# moved\npkg a::slow lib daily slow\npkg b::boot test:integration daily game-boot\n")
        self.assertEqual(ignored_tests.daily_rows(), [
            ignored_tests.Row("pkg", "a::slow", "lib", "daily", "tools/ci/ignored-tests/daily-pkg.txt", "slow"),
            ignored_tests.Row("pkg", "b::boot", "test:integration", "daily", "tools/ci/ignored-tests/daily-pkg.txt",
                              "game-boot"),
        ])
        self.assertEqual(ignored_tests.known_failures(), {})
        self.assertEqual(ignored_tests.harness_inputs(), {})
        for row in ("pkg a::slow lib daily", "pkg a::slow lib daily fast", "pkg a::slow tests/x.rs daily slow",
                    "pkg a::slow lib daily slow extra"):
            self.write("daily-pkg.txt", row + "\n")
            self.assertIn("PACKAGE TEST TARGET daily REASON", self.exit_message(ignored_tests.listed_entries))

    def test_plain_fixture_rows_are_inputs_not_known_failures(self):
        self.write("fixture.txt", "pkg a::ignored VAR needs\npkg b::plain VAR needs-plain\n")
        self.assertEqual(ignored_tests.known_failures(), {})
        self.assertEqual(ignored_tests.harness_inputs(), {
            ("pkg", "a::ignored"): "VAR", ("pkg", "b::plain"): "VAR"})
        self.assertEqual(ignored_tests.needs_commands("VAR", 2), [
            ["cargo", "test", "-j2", "-p", "pkg", "--lib", "--", "--ignored", "--exact", "a::ignored"],
            ["cargo", "test", "-j2", "-p", "pkg", "--lib", "--", "--exact", "b::plain"],
        ])

    def test_a_daily_row_needing_an_input_runs_with_that_input_never_in_a_shard(self):
        self.write("fixture.txt", "pkg a::ignored VAR needs\n")
        self.write("daily-pkg.txt", "pkg b::guest lib daily wasm-fixture\npkg c::slow lib daily slow\n"
                                    "pkg d::loads test:loads daily wasm-fixture\n"
                                    "sunrise-world f::shard lib daily wasm-fixture\n")
        variable = ignored_tests.DAILY_INPUTS["wasm-fixture"]
        self.write("other.txt", f"pkg e::other {variable} needs\n")
        # pkg has no shard: its rows get its workspace features, its library's too (a test behind a feature);
        # sunrise-world's shard builds its library plainly, so its row reuses that build.
        with patch.object(ignored_tests, "workspace_features",
                          return_value={"pkg": ["fixture"], "sunrise-world": ["extra"]}):
            self.assertEqual(ignored_tests.needs_commands(variable), [
                ["cargo", "test", "-p", "pkg", "--lib", "--", "--ignored", "--exact", "e::other"],
                ["cargo", "test", "-p", "pkg", "--features", "fixture", "--lib", "--", "--ignored", "--exact", "b::guest"],
                ["cargo", "test", "-p", "pkg", "--features", "fixture", "--test", "loads", "--", "--ignored", "--exact",
                 "d::loads"],
                ["cargo", "test", "-p", "sunrise-world", "--lib", "--", "--ignored", "--exact", "f::shard"],
            ])
        self.assertEqual([row.test for row in ignored_tests.daily_rows()], ["c::slow"])

    def test_an_unset_variable_runs_nothing(self):
        self.write("wasm-fixture.txt", "pkg a::one VAR needs\n")
        with patch.dict(os.environ), patch.object(ignored_tests.subprocess, "Popen") as popen:
            os.environ.pop("VAR", None)
            self.assertIn("VAR is unset", self.exit_message(lambda: ignored_tests.run_one_process_each("VAR")))
        popen.assert_not_called()

    def test_each_needs_test_runs_alone_and_must_report_one_pass(self):
        self.write("wasm-fixture.txt",
                   "pkg a::one VAR needs\npkg-lib b::two VAR needs-plain\npkg c::three VAR needs\npkg d::other OTHER needs\n")
        one_pass = "test result: ok. 1 passed; 0 failed; 0 ignored;\n"
        # Passes; matches nothing; reports its pass but the process exits non-zero (a crash after the summary).
        runs = iter([(one_pass, 0), ("test result: ok. 0 passed; 0 failed; 0 ignored;\n", 0), (one_pass, 101)])
        commands = []

        class Process:
            def __init__(self, command, **_):
                commands.append(command)
                output, self.code = next(runs)
                self.stdout = io.StringIO(output)

            def wait(self):
                return self.code

        with patch.dict(os.environ, {"VAR": "fixture"}), patch.object(ignored_tests.subprocess, "Popen", Process), \
                contextlib.redirect_stdout(io.StringIO()) as log:
            report = Path(self.folder.parent) / "report.json"
            self.assertEqual(ignored_tests.run_one_process_each("VAR", jobs=3, report=str(report)), 1)
        verdict = json.loads(report.read_text())
        self.assertEqual([(item["package"], item["test"]) for item in verdict["problems"]],
                         [("pkg-lib", "b::two"), ("pkg", "c::three")])
        self.assertEqual(verdict["passed"], [{"package": "pkg", "test": "a::one"}])
        self.assertEqual(commands, [
            ["cargo", "test", "-j3", "-p", "pkg", "--lib", "--", "--ignored", "--exact", "a::one"],
            ["cargo", "test", "-j3", "-p", "pkg-lib", "--lib", "--", "--exact", "b::two"],
            ["cargo", "test", "-j3", "-p", "pkg", "--lib", "--", "--ignored", "--exact", "c::three"],
        ])
        errors = [line for line in log.getvalue().splitlines() if line.startswith("::error::")]
        self.assertEqual(errors, [
            "::error::expected 1 passing test from: cargo test -j3 -p pkg-lib --lib -- --exact b::two",
            "::error::expected 1 passing test from: cargo test -j3 -p pkg --lib -- --ignored --exact c::three",
        ])

    def test_a_needs_test_the_workspace_build_made_runs_from_its_executable(self):
        self.write("wasm-fixture.txt", "pkg a::one VAR needs\npkg-lib b::two VAR needs-plain\n")
        one_pass = "test result: ok. 1 passed; 0 failed; 0 ignored;\n"
        ran, commands = [], []

        def run_binaries(package, executables, arguments, env):
            ran.append((package, executables, arguments))
            return subprocess.CompletedProcess([], 0, one_pass)

        class Process:
            def __init__(self, command, **_):
                commands.append(command)
                self.stdout = io.StringIO(one_pass)

            def wait(self):
                return 0

        with patch.dict(os.environ, {"VAR": "fixture"}), patch.object(ignored_tests.subprocess, "Popen", Process), \
                patch.object(ignored_tests, "run_binaries", run_binaries), contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(ignored_tests.run_one_process_each("VAR", jobs=3, binaries={("pkg", "lib"): "/t/pkg-1"}), 0)
        # Built: its executable, with the test's own selection; not built (pkg-lib): cargo, as before.
        self.assertEqual(ran, [("pkg", {"lib": "/t/pkg-1"}, ["--ignored", "--exact", "a::one"])])
        self.assertEqual(commands, [["cargo", "test", "-j3", "-p", "pkg-lib", "--lib", "--", "--exact", "b::two"]])

    def test_prebuilt_runs_the_named_targets_and_refuses_what_the_build_did_not_make(self):
        self.assertEqual(ignored_tests.cargo_targets(
            ["-p", "pkg", "--features", "x", "--lib", "--test", "it", "--no-fail-fast", "--", "name", "--ignored"]),
            ("pkg", ["lib", "test:it"], ["name", "--ignored"]))
        built = Path(self.folder.parent) / "built.json"
        built.write_text(json.dumps({"pkg lib": "/t/pkg-1", "pkg test:it": "/t/it-2"}))
        ran = []
        with patch.object(ignored_tests, "run_binaries",
                          lambda package, executables, arguments, env: ran.append((package, executables, arguments))
                          or subprocess.CompletedProcess([], 101, "test result: FAILED.\n")), \
                contextlib.redirect_stdout(io.StringIO()) as log:
            self.assertEqual(ignored_tests.run_prebuilt(str(built), ["-p", "pkg", "--test", "it", "--", "x"]), 101)
            self.assertEqual(ignored_tests.run_prebuilt(str(built), ["-p", "pkg", "--test", "gone"]), 101)
            self.assertEqual(ignored_tests.run_prebuilt(str(built) + ".missing", ["-p", "pkg", "--lib"]), 101)
        self.assertEqual(ran, [("pkg", {"test:it": "/t/it-2"}, ["x"])])
        self.assertIn("no test executable for pkg test:gone", log.getvalue())
        # A filter that matched no test (renamed, or behind a feature the build left off) fails, naming it.
        with patch.object(ignored_tests, "run_binaries", lambda *args: subprocess.CompletedProcess(
                [], 0, "running 0 tests\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 9 filtered out\n")), \
                contextlib.redirect_stdout(io.StringIO()) as log:
            self.assertEqual(ignored_tests.run_prebuilt(str(built), ["-p", "pkg", "--lib", "--", "guest_pool_"]), 101)
        self.assertIn("no test of pkg lib ran with guest_pool_", log.getvalue())
        with patch.object(ignored_tests, "run_binaries", lambda *args: subprocess.CompletedProcess(
                [], 0, "test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 9 filtered out\n")), \
                contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(ignored_tests.run_prebuilt(str(built), ["-p", "pkg", "--lib", "--", "guest_pool_"]), 0)


if __name__ == "__main__":
    unittest.main()
