"""Read one bounded history JSON member without extracting an artifact."""
import json
import sys
import zipfile

with zipfile.ZipFile(sys.argv[1]) as archive:
    members = archive.infolist()
    if len(members) != 1 or members[0].filename != 'ci-history.json' or members[0].file_size > 16 * 1024 * 1024:
        raise ValueError('unknown or over-budget history archive')
    value = json.loads(archive.read(members[0]))
    if value.get('version') != 1 or not isinstance(value.get('records'), list):
        raise ValueError('unknown history ledger')
    print(json.dumps(value))
