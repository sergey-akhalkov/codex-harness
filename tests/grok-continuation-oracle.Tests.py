"""Offline counterexamples for native Grok result acceptance."""
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('oracle', Path(__file__).with_name('grok-continuation-oracle.py'))
oracle = importlib.util.module_from_spec(spec)
spec.loader.exec_module(oracle)


def row(kind, **payload):
    return dict(type=kind, payload=payload)


def fixture():
    return [row('session_meta', id='fixture'), row('turn_context', model='xai/grok-4.6'),
            row('response_item', type='custom_tool_call', name='exec', call_id='a'),
            row('response_item', type='custom_tool_call_output', call_id='a', output='Script running with cell ID 1\nOutput:\n'),
            row('response_item', type='function_call', name='wait', call_id='b', arguments='{"cell_id":"1"}'),
            row('response_item', type='function_call_output', call_id='b', output=[{'type':'input_text','text':'Script completed\nOutput:\n'},{'type':'input_text','text':'MARKER'}]),
            row('event_msg', type='task_complete', last_agent_message='MARKER')]


class OracleTests(unittest.TestCase):
    def test_completed(self):
        self.assertEqual(oracle.verify(fixture(), 'MARKER')['waits'], ['1'])

    def test_empty_and_promised_final(self):
        for final in (None, '', 'I will wait', 'MARKER'):
            rows = fixture()[:4] + [row('event_msg', type='task_complete', last_agent_message=final)]
            with self.subTest(final=final), self.assertRaises(ValueError):
                oracle.verify(rows, 'MARKER')

    def test_wrong_handle(self):
        rows = fixture()
        rows[4]['payload']['arguments'] = '{"cell_id":"2"}'
        with self.assertRaises(ValueError): oracle.verify(rows, 'MARKER')

    def test_foreign_wait_and_malformed_arguments(self):
        rows = fixture()
        rows[4]['payload']['namespace'] = 'third_party'
        with self.assertRaises(ValueError): oracle.verify(rows, 'MARKER')
        rows = fixture()
        rows[4]['payload']['arguments'] = '[]'
        with self.assertRaises(ValueError): oracle.verify(rows, 'MARKER')

    def test_marker_must_come_from_result(self):
        rows = fixture()
        rows[5]['payload']['output'] = 'Script completed\nOutput:\n'
        rows.insert(0, row('response_item', type='message', role='user', content=[{'type':'input_text','text':'MARKER'}]))
        rows.insert(1, row('response_item', type='reasoning', encrypted_content='MARKER', summary=[{'text':'MARKER'}]))
        with self.assertRaises(ValueError): oracle.verify(rows, 'MARKER')

    def test_opaque_state_ignored(self):
        rows = fixture()
        rows.insert(2, row('response_item', type='reasoning', encrypted_content='not-decodable', content=None))
        self.assertEqual(oracle.verify(rows, 'MARKER')['final'], 'MARKER')

    def test_parse_error_keeps_pending(self):
        rows = fixture()
        rows[5]['payload']['output'] = 'failed to parse function arguments: invalid type: floating point'
        with self.assertRaises(ValueError): oracle.verify(rows, 'MARKER')

    def test_repeated_wait(self):
        rows = fixture()
        rows[5]['payload']['output'] = 'Script running with cell ID 1\nOutput:\n'
        rows[6:6] = [row('response_item', type='function_call', name='wait', call_id='c', arguments='{"cell_id":"1"}'),
                      row('response_item', type='function_call_output', call_id='c', output='Script completed\nMARKER')]
        self.assertEqual(oracle.verify(rows, 'MARKER')['waits'], ['1','1'])

    def test_missing_and_wrong_metadata(self):
        for rows in ([], fixture()[1:], fixture()[:-1], [{'payload':None}], fixture()+[row('turn_context', model='gpt-6-astra')]):
            with self.subTest(rows=rows), self.assertRaises(ValueError): oracle.verify(rows, 'MARKER')


if __name__ == '__main__':
    unittest.main()
