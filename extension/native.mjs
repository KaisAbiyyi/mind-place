export const NATIVE_HOST = 'com.mindplace.capture';

export function withTimeout(promise, timeoutMs) {
  let timer;
  const timeout = new Promise((_, reject) => {
    timer = setTimeout(() => reject(new Error('MIND_PLACE_TIMEOUT')), timeoutMs);
  });
  return Promise.race([promise, timeout]).finally(() => clearTimeout(timer));
}

export function validReceipt(receipt, expectedCompleteness, expectedCount) {
  return Boolean(receipt && receipt.ok === true
    && typeof receipt.id === 'string' && /^[a-f0-9]{64}$/i.test(receipt.id)
    && typeof receipt.snapshot_id === 'string' && /^[a-f0-9]{64}$/i.test(receipt.snapshot_id)
    && Number.isInteger(receipt.message_count) && receipt.message_count >= 0
    && receipt.message_count === expectedCount
    && typeof receipt.unchanged === 'boolean'
    && ['partial', 'complete'].includes(receipt.completeness)
    && receipt.completeness === expectedCompleteness
    && Array.isArray(receipt.warnings));
}
