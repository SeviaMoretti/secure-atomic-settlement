const fs = require('fs');

function readU32Leb(bytes, offset) {
  let result = 0;
  let shift = 0;
  let pos = offset;
  while (true) {
    const byte = bytes[pos];
    result |= (byte & 0x7f) << shift;
    pos += 1;
    if ((byte & 0x80) === 0) break;
    shift += 7;
  }
  return { value: result >>> 0, next: pos };
}

function writeU32Leb(value) {
  let v = value >>> 0;
  const out = [];
  do {
    let byte = v & 0x7f;
    v >>>= 7;
    if (v !== 0) byte |= 0x80;
    out.push(byte);
  } while (v !== 0);
  return Buffer.from(out);
}

function readName(bytes, offset) {
  const len = readU32Leb(bytes, offset);
  const start = len.next;
  const end = start + len.value;
  return { value: Buffer.from(bytes.slice(start, end)).toString('utf8'), next: end };
}

function writeName(name) {
  const buf = Buffer.from(name, 'utf8');
  return Buffer.concat([writeU32Leb(buf.length), buf]);
}

function parseExportEntries(payload) {
  const count = readU32Leb(payload, 0);
  let pos = count.next;
  const entries = [];
  for (let i = 0; i < count.value; i += 1) {
    const name = readName(payload, pos);
    pos = name.next;
    const kind = payload[pos];
    pos += 1;
    const index = readU32Leb(payload, pos);
    pos = index.next;
    entries.push({ name: name.value, kind, index: index.value });
  }
  return entries;
}

function encodeExportEntries(entries) {
  const parts = [writeU32Leb(entries.length)];
  for (const entry of entries) {
    parts.push(writeName(entry.name));
    parts.push(Buffer.from([entry.kind]));
    parts.push(writeU32Leb(entry.index));
  }
  return Buffer.concat(parts);
}

function rewriteExports(input, output, allowedNames) {
  const bytes = fs.readFileSync(input);
  const magicVersion = bytes.slice(0, 8);
  if (magicVersion.toString('hex') !== '0061736d01000000') {
    throw new Error('input is not a wasm module');
  }

  const chunks = [magicVersion];
  let pos = 8;
  let rewrote = false;
  while (pos < bytes.length) {
    const id = bytes[pos];
    pos += 1;
    const size = readU32Leb(bytes, pos);
    pos = size.next;
    const payload = bytes.slice(pos, pos + size.value);
    pos += size.value;

    if (id === 7) {
      const entries = parseExportEntries(payload);
      const filtered = entries.filter((entry) => allowedNames.has(entry.name));
      const newPayload = encodeExportEntries(filtered);
      chunks.push(Buffer.from([id]), writeU32Leb(newPayload.length), newPayload);
      rewrote = true;
    } else {
      chunks.push(Buffer.from([id]), writeU32Leb(payload.length), payload);
    }
  }

  if (!rewrote) {
    throw new Error('export section not found');
  }
  fs.writeFileSync(output, Buffer.concat(chunks));
}

const [input, output, ...names] = process.argv.slice(2);
if (!input || !output || names.length === 0) {
  console.error('usage: node strip_wasm_exports.js <input.wasm> <output.wasm> <export> [export...]');
  process.exit(2);
}

rewriteExports(input, output, new Set(names));
