/** Password generator (crypto-backed). Matches web UI charset defaults. */

const LOWER = "abcdefghijklmnopqrstuvwxyz";
const UPPER = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const NUMS = "0123456789";
const SYMS = "!@#$%^&*()_+~`|}{[]:;?><,./-=";

/**
 * @param {object} [opts]
 * @param {number} [opts.length=24]
 * @param {boolean} [opts.useUpper=true]
 * @param {boolean} [opts.useNumbers=true]
 * @param {boolean} [opts.useSymbols=true]
 */
export function generatePassword({
  length = 24,
  useUpper = true,
  useNumbers = true,
  useSymbols = true,
} = {}) {
  const len = Math.max(4, Math.min(128, Number(length) || 24));
  let chars = LOWER;
  if (useUpper) chars += UPPER;
  if (useNumbers) chars += NUMS;
  if (useSymbols) chars += SYMS;
  if (!chars) chars = LOWER;

  const out = new Array(len);
  const max = 256 - (256 % chars.length);
  let i = 0;
  while (i < len) {
    const buf = new Uint8Array(len - i);
    crypto.getRandomValues(buf);
    for (const b of buf) {
      if (b >= max) continue;
      out[i++] = chars[b % chars.length];
      if (i >= len) break;
    }
  }
  return out.join("");
}
