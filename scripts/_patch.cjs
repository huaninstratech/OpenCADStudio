// Generic patch runner: reads a JSON spec {file, find, replace}[] and applies
// literal (non-regex) replacements. Used for quick surgical edits.
const fs = require('fs');
const specPath = process.argv[2];
if (!specPath) { console.error('usage: node _patch.cjs <spec.json>'); process.exit(1); }
const spec = JSON.parse(fs.readFileSync(specPath, 'utf8'));
for (const { file, find, replace, all } of spec) {
    let s = fs.readFileSync(file, 'utf8');
    if (!s.includes(find)) { console.error('ANCHOR MISSING in ' + file + ':\n' + find.slice(0, 120)); process.exit(1); }
    if (all) {
        while (s.includes(find)) { s = s.replace(find, replace); }
    } else {
        s = s.replace(find, replace);
    }
    fs.writeFileSync(file, s);
}
console.log('patched ' + spec.length + ' hunk(s)');
