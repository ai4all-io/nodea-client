// Renders the production npm dependency tree's licences as Markdown on stdout.
// Input: the JSON that `license-checker --production --json` writes, on stdin.
// Output is deterministic (sorted, no absolute paths, no dates) so CI can diff it.

import { readFileSync } from 'node:fs';
import { basename } from 'node:path';

const ROOT_PACKAGE = JSON.parse(readFileSync('package.json', 'utf8')).name;

const MIT_TEXT = `Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.`;

// Used for a package that ships no licence file of its own.
const FALLBACK_TEXT = {
  MIT: MIT_TEXT,
  'Apache-2.0': 'The full text of the Apache License 2.0 is in LICENSE.',
  'Apache-2.0 OR MIT': `Used under the MIT licence:\n\n${MIT_TEXT}`,
  'CC0-1.0': 'Dedicated to the public domain under CC0 1.0: https://creativecommons.org/publicdomain/zero/1.0/legalcode',
};

const isLicenceFile = (path) => /^(licen[cs]e|copying)/i.test(basename(path ?? ''));

const input = JSON.parse(readFileSync(0, 'utf8'));
const entries = Object.entries(input)
  .filter(([id]) => !id.startsWith(`${ROOT_PACKAGE}@`))
  .sort(([a], [b]) => a.localeCompare(b));

const out = ['## npm packages (frontend bundle)', ''];
for (const [id, info] of entries) {
  out.push(`### ${id}`, '', `License: ${info.licenses}`);
  if (info.repository) out.push(`Source: ${info.repository}`);
  out.push('');
  let text;
  if (isLicenceFile(info.licenseFile)) {
    text = readFileSync(info.licenseFile, 'utf8').trim();
  } else {
    text = FALLBACK_TEXT[info.licenses];
    if (!text) throw new Error(`${id}: no licence file and no fallback text for ${info.licenses}`);
    if (info.publisher) out.push(`Copyright (c) ${info.publisher}`, '');
  }
  out.push('```text', text.replace(/```/g, "'''"), '```', '');
}
process.stdout.write(out.join('\n'));
