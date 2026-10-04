import copy
from datetime import date
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location('quarantine', Path(__file__).parents[1] / 'ci-quarantine.py')
policy = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(policy)


class QuarantinePolicy(unittest.TestCase):
    def test_missing_metadata_expiration_and_unregistered_tags_fail_closed(self):
        entry = {'id':'control', 'suite':'web', 'file':'web/e2e/control.spec.ts', 'title':'exact scenario', 'oses':['Linux'],
            'owner':'@owner', 'issue':'https://example.com/issue', 'evidence':'https://example.com/run',
            'registered':'2026-10-04', 'expires':'2026-10-11', 'signature':{'category':'assertion','any_of':[['Expected: 3','Received: 2']]},
            'fix_paths':['web/src/contract.ts'], 'alternative':['web/e2e/alternative.spec.ts'], 'return_criteria':'30 independent retry-zero fixtures and original suite'}
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'contracts').mkdir()
            (root / 'web/e2e').mkdir(parents=True)
            (root / 'web/e2e/control.spec.ts').write_text('test("exact scenario", {tag:"@flaky", issue:"https://example.com/issue"}, () => {});')
            (root / 'web/e2e/alternative.spec.ts').write_text('test("alternative", () => {});')
            registry = root / 'contracts/ci-quarantine.json'
            def validate(value=entry, today=date(2026,10,4)):
                registry.write_text(json.dumps({'version':1,'entries':[value]}))
                return policy.validate(root, today)
            self.assertEqual(validate(), [entry])
            for field in ('owner','issue','evidence','expires','signature','alternative','return_criteria'):
                value = copy.deepcopy(entry)
                del value[field]
                with self.subTest(field=field), self.assertRaises(ValueError):
                    validate(value)
            with self.assertRaisesRegex(ValueError, 'expired'):
                validate(today=date(2026,10,11))
            (root / 'web/e2e/alternative.spec.ts').write_text('test("unregistered", {tag:"@flaky"}, () => {});')
            with self.assertRaisesRegex(ValueError, 'unregistered'):
                validate()

    def test_unknown_selection_and_missing_or_skipped_observations_fail(self):
        entry = {'id':'control','suite':'web','file':'web/e2e/test.spec.ts','title':'exact','oses':['Linux','macOS']}
        with self.assertRaisesRegex(ValueError, 'unknown'):
            policy.selection([entry], 'web', 'Linux', 'required', {'quarantine_required':['not-registered']})
        for status in (None, 'skipped','unknown','interrupted','failed'):
            records = [] if status is None else [{'suite':entry['file'],'test':entry['title'],'os':'Linux','retry':0,'status':status}]
            with self.subTest(status=status), self.assertRaises(ValueError):
                policy.results([entry], {'records':records}, True, 'Linux')

        with self.assertRaisesRegex(ValueError, 'missing'):
            policy.results([entry], {'records':[{'suite':entry['file'],'test':entry['title'],'os':'macOS','retry':0,'status':'passed'}]}, True, 'Linux')


if __name__ == '__main__':
    unittest.main()
