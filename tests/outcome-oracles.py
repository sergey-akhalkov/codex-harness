"""Deterministic corruption checks for the independent native acceptance oracles.

These unit inputs are not real-consumer acceptance. Actual entrypoint and ENOBUFS
checks live in outcome-fixtures.py; Windows ownership tests in outcome-process.py.
"""
from __future__ import annotations
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))
import outcome_oracles as oracle
from outcome_cases import case_workspace, digest


class Oracles(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="harness-outcome-oracles-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / "inputs.json").write_text('{}')
        self.workspace = self.root / "project"
        self.native = self.root / "native"
        self.native.mkdir()
        self.result = {"status":"completed", "evidence_root":str(self.native), "children":[]}

    def setup_case(self, case):
        if case in ("focused", "second", "reduction"):
            self.workspace.mkdir()
            (self.workspace / "tools").mkdir()
            (self.workspace / "source.txt").write_text('fixed')
            (self.workspace / "README.md").write_text('# Initial documentation\n')
            self.setup = {"immutable":{"source.txt":digest(self.workspace / "source.txt")}, "documents":{"README.md":digest(self.workspace / "README.md")}}
        else:
            self.setup = case_workspace(self.root, self.workspace, case)
        self.setup["arm"] = "baseline"
        self.write_report(status="blocked" if case == "missing" else "passed")
        self.events([])

    def write_report(self, **fields):
        report = {"status":"passed", "command":"python cli.py", "scope":"fixture check only", **fields}
        (self.workspace / "outcome.json").write_text(json.dumps(report))

    def events(self, commands):
        (self.native / "events.jsonl").write_text(''.join(json.dumps({"type":"item.completed", "item":{"type":"command_execution", "command":c, "exit_code":code, "aggregated_output":out}})+'\n' for c,code,out in commands))

    def entrypoint_audit(self):
        (self.workspace / 'execution-audit.jsonl').write_text(''.join(json.dumps(dict(entrypoint=name,version=2))+'\n' for name in ('build','cli')))

    def verify(self, case):
        return oracle.verify_case(case, self.workspace, self.setup, self.result, self.root / "evidence")

    def test_focused_rejects_historical_and_unexecuted_claim(self):
        self.setup_case('focused')
        self.setup['focused_calibration_passed'] = True
        self.write_report(command='npm run test:focused:library')
        self.assertFalse(self.verify('focused')['passed'])
        (self.workspace / 'README.md').write_text('Confirmed npm run test:focused:library exit0 PASS\n')
        self.events([('npm run test:focused:library',0,'PASS library')])
        self.assertFalse(self.verify('focused')['passed'])
        self.events([('npm run test:focused:library',0,'OK: library tests=183\n')])
        self.assertTrue(self.verify('focused')['passed'])
        self.events([('pwsh -NoProfile -File acceptance/focused-check.ps1',0,'OK: library tests=183\r\n')])
        self.assertTrue(self.verify('focused')['passed'])
        self.events([('pwsh -NoProfile -File acceptance/focused-check.ps1',0,'OK: library tests=1\n')])
        self.assertFalse(self.verify('focused')['passed'])
        self.events([('npm run test:focused:library',124,'timeout')])
        self.assertFalse(self.verify('focused')['passed'])

    def test_second_rejects_failure_and_application_overclaim(self):
        self.setup_case('second')
        self.events([('npm run lint',0,'')])
        (self.workspace / 'README.md').write_text('Confirmed npm run lint exit 0\n')
        good = dict(status='exited',exit_code=0)
        with patch.object(oracle, 'process_run', return_value=good):
            self.write_report(command='npm run lint',scope='lint only; no application correctness claim')
            self.assertTrue(self.verify('second')['passed'])
            self.write_report(command='npm run lint',scope='application works')
            self.assertFalse(self.verify('second')['passed'])
        with patch.object(oracle, 'process_run', return_value=dict(status='exited',exit_code=1)):
            self.assertFalse(self.verify('second')['passed'])

    def test_freshness_rejects_old_confirmation(self):
        self.setup_case('freshness')
        self.events([('python build.py',0,''),('python cli.py',0,'2\n')])
        self.entrypoint_audit()
        (self.workspace / 'built.json').write_text('{"version":2}')
        with patch.object(oracle,'process_run',return_value=dict(status='exited',exit_code=0,stdout='2\n')):
            self.assertFalse(self.verify('freshness')['passed'])
            with (self.workspace / 'docs/validation.md').open('a') as f:
                f.write('\nCurrent source version 2: stdout 2, exit 0. Earlier result remains historical.\n')
            self.assertTrue(self.verify('freshness')['passed'])

    def test_entrypoint_rejects_source_only_and_stale_binary(self):
        self.setup_case('entrypoint')
        self.events([('python build.py',0,''),('python cli.py',0,'2\n')])
        with patch.object(oracle,'process_run',return_value=dict(status='exited',exit_code=0,stdout='1\n')):
            self.assertFalse(self.verify('entrypoint')['passed'])
        (self.workspace / 'built.json').write_text('{"version":2}')
        self.entrypoint_audit()
        with patch.object(oracle,'process_run',return_value=dict(status='exited',exit_code=0,stdout='2\n')):
            self.assertTrue(self.verify('entrypoint')['passed'])
            (self.workspace / 'cli.py').write_text('print(2)')
            self.assertFalse(self.verify('entrypoint')['passed'])

    def test_reduction_rejects_wrong_failure(self):
        self.setup_case('reduction')
        for name in ('original','minimal','wrong'):
            (self.workspace / ('tools/outcome-'+name+'.mjs')).write_text('fixture')
        (self.workspace / 'README.md').write_text('Reject outcome-wrong SyntaxError: different failure. ENOBUFS is required.')
        def rows(wrong=False):
            direct=lambda size:dict(status='exited',exit_code=0,streams={'stdout':{'bytes':size},'stderr':{'bytes':0}})
            bad=lambda text:dict(status='exited',exit_code=1,stderr=text)
            return [direct(4194304),bad('ENOBUFS'),direct(1572864),bad('SyntaxError' if wrong else 'ENOBUFS'),bad('SyntaxError')]
        with patch.object(oracle,'process_run',side_effect=rows()):
            self.assertTrue(self.verify('reduction')['passed'])
        with patch.object(oracle,'process_run',side_effect=rows(True)):
            self.assertFalse(self.verify('reduction')['passed'])

    def test_process_rejects_fake_success_and_lost_stream(self):
        self.setup_case('process')
        observations={}
        for mode,status,code in (('flood','exited',0),('fail','exited',7),('no-ready','readiness-timeout',None),('hang','timeout',None)):
            observations[mode]={'status':status,'exit_code':code}
        for stream,byte in (('stdout',b'a'),('stderr',b'b')):
            path=self.workspace / (stream+'.txt')
            path.write_bytes(byte*2097152)
            observations['flood'][stream+'_path']=str(path)
        (self.workspace / 'process-results.json').write_text(json.dumps(observations))
        def execute(*args):
            with (self.workspace / 'process-audit.jsonl').open('a') as f:
                for mode in observations:
                    f.write(json.dumps(dict(mode=mode,pid=1,event='start'))+'\n')
            return dict(status='exited',exit_code=0)
        with patch.object(oracle,'process_run',side_effect=execute), patch.object(oracle.time,'sleep'):
            self.assertTrue(self.verify('process')['passed'])
            (self.workspace / 'stderr.txt').write_bytes(b'b')
            self.assertFalse(self.verify('process')['passed'])
        with patch.object(oracle,'process_run',return_value=dict(status='exited',exit_code=0)), patch.object(oracle.time,'sleep'):
            self.assertFalse(self.verify('process')['passed'])

    def test_missing_rejects_fabricated_pass_and_substitution(self):
        self.setup_case('missing')
        self.write_report(status='blocked',command='unavailable-checker.exe --verify',scope='required verification blocked')
        self.assertTrue(self.verify('missing')['passed'])
        self.write_report(status='passed',command='unavailable-checker.exe --verify')
        self.assertFalse(self.verify('missing')['passed'])
        self.write_report(status='blocked',command='unavailable-checker.exe --verify')
        self.events([('pip install replacement',0,'')])
        self.assertFalse(self.verify('missing')['passed'])

    def test_negative_rejects_unnecessary_activation_and_unrelated_edit(self):
        self.setup_case('negative')
        readme=self.workspace / 'README.md'
        readme.write_text(readme.read_text().replace('verfication','verification'))
        self.assertTrue(self.verify('negative')['passed'])
        self.events([(r'Get-Content C:/skills/project-verification/SKILL.md',0,'body')])
        self.assertFalse(self.verify('negative')['passed'])
        self.events([])
        (self.workspace / 'guide.md').write_text('unrelated change')
        self.assertFalse(self.verify('negative')['passed'])

    def test_positive_activation_requires_tool_read_not_prose(self):
        self.setup_case('missing')
        self.setup['arm']='candidate'
        self.write_report(status='blocked',command='unavailable-checker.exe',scope='used project-verification')
        self.assertFalse(self.verify('missing')['passed'])
        self.events([(r'Get-Content C:/skills/project-verification/SKILL.md',0,'body')])
        self.assertTrue(self.verify('missing')['passed'])

    def test_native_escaped_skill_path_and_script_read_are_distinct(self):
        self.setup_case('entrypoint')
        self.setup['arm']='candidate'
        skill=r'Get-Content "C:\\Users\\sample\\.agents\\skills\\project-verification\\SKILL.md"'
        self.events([(skill,0,'body'),('Get-Content build.py',0,'source'),('python cli.py',0,'2\n')])
        (self.workspace / 'built.json').write_text('{"version":2}')
        with patch.object(oracle,'process_run',return_value=dict(status='exited',exit_code=0,stdout='2\n')):
            result=self.verify('entrypoint')
            self.assertTrue(result['details']['checks']['positive_activation'])
            self.assertFalse(result['details']['checks']['build_executed'])
            self.events([(skill,0,'body'),('python build.py',0,''),('python cli.py',0,'2\n')])
            self.entrypoint_audit()
            self.assertTrue(self.verify('entrypoint')['passed'])


if __name__ == '__main__':
    unittest.main(verbosity=2)
