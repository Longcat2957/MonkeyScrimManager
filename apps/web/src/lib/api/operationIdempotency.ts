type IdempotencyKeyFactory = () => string;

interface PendingOperation {
	fingerprint: string;
	key: string;
}

export interface OperationIdempotencyKeys {
	keyFor(operation: string, payload: unknown): string;
	complete(operation: string): void;
	invalidate(operation: string): void;
}

export function createOperationIdempotencyKeys(
	keyFactory: IdempotencyKeyFactory = () => globalThis.crypto.randomUUID()
): OperationIdempotencyKeys {
	const pending = new Map<string, PendingOperation>();

	return {
		keyFor(operation, payload) {
			const fingerprint = JSON.stringify(payload);
			const current = pending.get(operation);
			if (current?.fingerprint === fingerprint) return current.key;

			const next = { fingerprint, key: keyFactory() };
			pending.set(operation, next);
			return next.key;
		},
		complete(operation) {
			pending.delete(operation);
		},
		invalidate(operation) {
			pending.delete(operation);
		}
	};
}
