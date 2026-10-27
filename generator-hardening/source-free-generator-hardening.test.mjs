import test from "node:test";
import assert from "node:assert/strict";

const reserved = new Set([
  "import","from","as","extends","implements","try","catch","throw","raise","panic",
  "recover","finally","end","fi","if","do","else","then","new","spawn","stop","done",
  "await","rt","async","suspend","suspends","critical","gen","generator","nlex","trap",
  "actor","isoactor","def","fnc","routine","while","for","of","loop","block","break",
  "continue","yield","super","elseif","elif","switch","match","matches","is","default",
  "eq","neq","nb","select","readch","writech","cb","shared","untrusted","type","types",
  "typeof","interface","trait","struct","impl","abstract","void","static","pub","private",
  "return","defer","val","const","let","mut","borrow","self","true","false","null","obj",
  "arr","Iterator","AsyncIterator","int","define","class","enum","pattern","module",
  "contract","conforms","namespace","gpu","cpu","quantum","over","structural","bool","pure",
  "Some","None","Ok","Err","using","requires","serial","critical-read","critical-write","nb-select"
]);

const newlyClosed = [
  "define","class","enum","pattern","module","contract","conforms","namespace",
  "gpu","cpu","quantum","over","structural","bool","pure","Some","None","Ok","Err",
  "using","requires","serial","critical-read","critical-write","nb-select"
];

function generatorIdentifier(name) {
  if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(name) || reserved.has(name))
    throw new Error("reserved generator identifier");
  return name;
}

function admitGenerator(catalog, name, max = 512) {
  if (catalog.length >= max) throw new Error("generator declaration limit exceeded");
  generatorIdentifier(name);
  if (catalog.includes(name)) throw new Error("duplicate generator name/arity");
  return [...catalog, name];
}

test("reserved-name parity closes all newly audited names", () => {
  assert.equal(reserved.size, 115);
  for (const name of newlyClosed) assert.ok(reserved.has(name), name);
  for (const name of newlyClosed.filter(x => /^[A-Za-z_][A-Za-z0-9_]*$/.test(x)))
    assert.throws(() => generatorIdentifier(name), /reserved generator identifier/, name);
});

test("duplicate zero-arity generator ABI slots fail closed", () => {
  const first = admitGenerator([], "values");
  assert.throws(() => admitGenerator(first, "values"), /duplicate generator name\/arity/);
});

test("declaration budget accepts 512 and rejects 513", () => {
  let catalog = [];
  for (let i = 0; i < 512; i++) catalog = admitGenerator(catalog, "g" + i);
  assert.equal(catalog.length, 512);
  assert.throws(() => admitGenerator(catalog, "g512"), /declaration limit exceeded/);
});

test("ordinary non-reserved identifiers remain admitted", () => {
  for (const name of ["values","bounds","telemetry_2","G"])
    assert.equal(generatorIdentifier(name), name);
});
