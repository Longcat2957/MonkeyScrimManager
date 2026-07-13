import { describe, expect, it, vi } from 'vitest';
import { createOperationIdempotencyKeys } from './operationIdempotency';

describe('operation idempotency keys', () => {
	it('keeps a key for an identical retry and replaces it when the payload changes', () => {
		const keyFactory = vi.fn().mockReturnValueOnce('key-1').mockReturnValueOnce('key-2');
		const keys = createOperationIdempotencyKeys(keyFactory);

		expect(keys.keyFor('save-user', { name: 'A', revision: 1 })).toBe('key-1');
		expect(keys.keyFor('save-user', { name: 'A', revision: 1 })).toBe('key-1');
		expect(keys.keyFor('save-user', { name: 'B', revision: 1 })).toBe('key-2');
		expect(keyFactory).toHaveBeenCalledTimes(2);
	});

	it('starts a new operation after success without affecting other pending operations', () => {
		const keyFactory = vi
			.fn()
			.mockReturnValueOnce('user-1')
			.mockReturnValueOnce('group-1')
			.mockReturnValueOnce('user-2');
		const keys = createOperationIdempotencyKeys(keyFactory);

		expect(keys.keyFor('save-user', { revision: 1 })).toBe('user-1');
		expect(keys.keyFor('save-group', { revision: 3 })).toBe('group-1');
		keys.complete('save-user');

		expect(keys.keyFor('save-user', { revision: 1 })).toBe('user-2');
		expect(keys.keyFor('save-group', { revision: 3 })).toBe('group-1');
	});

	it('drops a pending key as soon as the UI invalidates changed input', () => {
		const keyFactory = vi.fn().mockReturnValueOnce('before').mockReturnValueOnce('after');
		const keys = createOperationIdempotencyKeys(keyFactory);

		expect(keys.keyFor('participants', { userIds: ['a'] })).toBe('before');
		keys.invalidate('participants');
		expect(keys.keyFor('participants', { userIds: ['a'] })).toBe('after');
	});
});
