#!/usr/bin/env python3
"""M0-02 static contract presence check. NOT a Rust compiler or test runner."""
from pathlib import Path
import re
import tomllib

root=Path(__file__).resolve().parents[1]
base=root/'crates/core-domain'
manifest=tomllib.loads((base/'Cargo.toml').read_text())
assert set(manifest['dependencies'])=={'serde','serde_json','sha2','unicode-normalization'}
for f in ['lib.rs','identity.rs','record.rs','metric.rs','version.rs','error.rs']:
    assert (base/'src'/f).is_file(),f
record=(base/'src/record.rs').read_text()
fields=re.search(r'pub struct CanonicalRecord\s*\{(.*?)\n\}',record,re.S).group(1)
assert 'pub record_revision' not in fields and 'pub payload_hash' not in fields
assert all(s in fields for s in ['pub record_id:', 'pub owner_device_id:', 'pub usage:', 'pub scope:'])
version=(base/'src/version.rs').read_text()
assert 'pub record_revision:' in version and 'pub payload_hash:' in version
assert 'canonical-record/v1' in version
metric=(base/'src/metric.rs').read_text()
assert all(x in metric for x in ['checked_add','aggregate_device_local','priced_records','unpriced_records'])
assert 'UsageScope::DeviceLocal' in metric
identity=(base/'src/identity.rs').read_text()
assert 'len.to_be_bytes()' in identity and 'stable_record_id' in identity
code=(base/'tests/domain_contract.rs').read_text()
count=len(re.findall(r'^#\[test\]',code,re.M))
assert count>=50, count
assert (root/'fixtures/domain/canonical-record-v1.json').is_file()
print(f'PASS M0-02 static contract shape ({count} Rust test cases present; no build verification)')
