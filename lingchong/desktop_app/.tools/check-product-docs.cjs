const fs = require('node:fs');
const path = require('node:path');
const root = path.resolve(__dirname, '..');
const files = ['README.md', 'docs/user-manual.md', 'docs/roadmap.md', 'docs/windows-guide.md', 'docs/configuration.md'];
const slug = value => value.toLowerCase().replace(/[^\p{L}\p{N}_\-\s]/gu, '').replace(/\s/g, '-');
let count = 0;
for (const name of files) {
  const filename = path.join(root, name);
  const content = fs.readFileSync(filename, 'utf8');
  if (content.includes('\ufffd')) throw new Error(`Invalid encoding: ${name}`);
  const fences = content.match(/^```/gm) || [];
  if (fences.length % 2) throw new Error(`Unclosed code fence: ${name}`);
  for (const match of content.matchAll(/\[[^\]]+\]\(([^)]+)\)/g)) {
    const target = match[1];
    if (/^[a-z]+:/i.test(target)) continue;
    const [file, anchor] = target.split('#');
    const resolved = file ? path.resolve(path.dirname(filename), file) : filename;
    if (!fs.existsSync(resolved)) throw new Error(`Broken link in ${name}: ${target}`);
    if (anchor) {
      const headings = [...fs.readFileSync(resolved, 'utf8').matchAll(/^#{1,6}\s+(.+)$/gm)].map(match => slug(match[1]));
      if (!headings.includes(decodeURIComponent(anchor))) throw new Error(`Broken heading in ${name}: ${target}`);
    }
    count++;
  }
  console.log(`${name}: ${content.split('\n').length} lines, valid UTF-8/fences`);
}
console.log(`PASS: ${count} local documentation links and heading anchors checked.`);
