import { z } from 'zod';
import type { RealtimeConnectionState } from './types';

const realtimeInvalidateEventSchema = z
	.object({
		type: z.literal('invalidate'),
		eventId: z.uuid(),
		topic: z.string().min(1),
		resourceId: z.string().min(1),
		revision: z.number().int().nonnegative(),
		occurredAt: z.iso.datetime({ offset: true })
	})
	.strict();

const realtimeSubscribedEventSchema = z
	.object({
		type: z.literal('subscribed'),
		topics: z.array(z.string().min(1))
	})
	.passthrough();

const realtimeResyncRequiredEventSchema = z
	.object({
		type: z.literal('resync_required'),
		topics: z.array(z.string().min(1)).optional()
	})
	.passthrough();

const realtimeErrorEventSchema = z
	.object({
		type: z.literal('error'),
		code: z.string().min(1),
		message: z.string().min(1)
	})
	.passthrough();

export type RealtimeInvalidateEvent = z.infer<typeof realtimeInvalidateEventSchema>;

export interface RealtimeWebSocket {
	readonly readyState: number;
	onopen: ((event: Event) => void) | null;
	onclose: ((event: CloseEvent) => void) | null;
	onerror: ((event: Event) => void) | null;
	onmessage: ((event: MessageEvent) => void) | null;
	send(data: string): void;
	close(code?: number, reason?: string): void;
}

interface RealtimeSocketRuntime {
	createWebSocket(url: string): RealtimeWebSocket;
	setTimeout(callback: () => void, delayMs: number): ReturnType<typeof globalThis.setTimeout>;
	clearTimeout(timer: ReturnType<typeof globalThis.setTimeout>): void;
	isOnline(): boolean;
	addOnlineListener(listener: () => void): void;
	removeOnlineListener(listener: () => void): void;
	addOfflineListener(listener: () => void): void;
	removeOfflineListener(listener: () => void): void;
	random(): number;
}

export interface BrowserRealtimeSocketOptions {
	topics: readonly string[];
	onInvalidate(event: RealtimeInvalidateEvent): void;
	onResyncRequired?(): void;
	onProtocolError?(error: { code: string; message: string }): void;
	onConnectionStateChange?(state: RealtimeConnectionState): void;
	url?: string;
	retry?: {
		initialDelayMs?: number;
		maxDelayMs?: number;
	};
	runtime?: Partial<RealtimeSocketRuntime>;
}

const socketOpen = 1;
const defaultInitialDelayMs = 500;
const defaultMaxDelayMs = 15_000;

function browserRuntime(): RealtimeSocketRuntime {
	return {
		createWebSocket(url) {
			return new globalThis.WebSocket(url);
		},
		setTimeout(callback, delayMs) {
			return globalThis.setTimeout(callback, delayMs);
		},
		clearTimeout(timer) {
			globalThis.clearTimeout(timer);
		},
		isOnline() {
			return globalThis.navigator?.onLine ?? true;
		},
		addOnlineListener(listener) {
			globalThis.addEventListener?.('online', listener);
		},
		removeOnlineListener(listener) {
			globalThis.removeEventListener?.('online', listener);
		},
		addOfflineListener(listener) {
			globalThis.addEventListener?.('offline', listener);
		},
		removeOfflineListener(listener) {
			globalThis.removeEventListener?.('offline', listener);
		},
		random() {
			return globalThis.Math.random();
		}
	};
}

function resolveRuntime(
	overrides: Partial<RealtimeSocketRuntime> | undefined
): RealtimeSocketRuntime {
	return { ...browserRuntime(), ...overrides };
}

export function sameOriginWebSocketUrl(location: Pick<Location, 'origin' | 'protocol'>): string {
	const url = new URL('/ws', location.origin);
	url.protocol = location.protocol === 'https:' ? 'wss:' : 'ws:';
	return url.toString();
}

export class BrowserRealtimeSocket {
	readonly topics: readonly string[];

	#socket: RealtimeWebSocket | null = null;
	#retryTimer: ReturnType<typeof globalThis.setTimeout> | null = null;
	#retryAttempt = 0;
	#disposed = false;
	#listening = false;
	#connectionState: RealtimeConnectionState = 'offline';
	#runtime: RealtimeSocketRuntime;
	#url: string;
	#onInvalidate: (event: RealtimeInvalidateEvent) => void;
	#onResyncRequired: (() => void) | undefined;
	#onProtocolError: ((error: { code: string; message: string }) => void) | undefined;
	#onConnectionStateChange: ((state: RealtimeConnectionState) => void) | undefined;
	#initialDelayMs: number;
	#maxDelayMs: number;

	constructor(options: BrowserRealtimeSocketOptions) {
		this.topics = [...new Set(options.topics)];
		this.#runtime = resolveRuntime(options.runtime);
		this.#url = options.url ?? sameOriginWebSocketUrl(globalThis.location);
		this.#onInvalidate = options.onInvalidate;
		this.#onResyncRequired = options.onResyncRequired;
		this.#onProtocolError = options.onProtocolError;
		this.#onConnectionStateChange = options.onConnectionStateChange;
		this.#initialDelayMs = options.retry?.initialDelayMs ?? defaultInitialDelayMs;
		this.#maxDelayMs = options.retry?.maxDelayMs ?? defaultMaxDelayMs;
	}

	get connectionState(): RealtimeConnectionState {
		return this.#connectionState;
	}

	connect(): void {
		if (this.#disposed) return;
		if (!this.#listening) {
			this.#runtime.addOnlineListener(this.#handleOnline);
			this.#runtime.addOfflineListener(this.#handleOffline);
			this.#listening = true;
		}
		this.#open();
	}

	reconnect(): void {
		if (this.#disposed) return;
		this.#clearRetryTimer();
		this.#retryAttempt = 0;
		this.#closeSocket();
		this.#open();
	}

	disconnect(): void {
		if (this.#disposed) return;
		this.#disposed = true;
		this.#clearRetryTimer();
		if (this.#listening) {
			this.#runtime.removeOnlineListener(this.#handleOnline);
			this.#runtime.removeOfflineListener(this.#handleOffline);
			this.#listening = false;
		}
		this.#closeSocket();
	}

	#handleOnline = () => {
		if (this.#disposed) return;
		this.reconnect();
	};

	#handleOffline = () => {
		if (this.#disposed) return;
		this.#clearRetryTimer();
		this.#closeSocket();
		this.#setConnectionState('offline');
	};

	#open(): void {
		if (this.#disposed) return;
		if (!this.#runtime.isOnline()) {
			this.#setConnectionState('offline');
			return;
		}
		if (this.#socket) return;

		this.#setConnectionState('reconnecting');
		let socket: RealtimeWebSocket;
		try {
			socket = this.#runtime.createWebSocket(this.#url);
		} catch {
			this.#scheduleReconnect();
			return;
		}

		this.#socket = socket;
		socket.onopen = () => {
			if (this.#disposed || this.#socket !== socket) return;
			this.#sendSubscribe(socket);
		};
		socket.onmessage = (message) => {
			if (this.#disposed || this.#socket !== socket) return;
			this.#handleMessage(message);
		};
		socket.onerror = () => {
			if (this.#disposed || this.#socket !== socket) return;
			this.#setConnectionState('reconnecting');
		};
		socket.onclose = () => {
			if (this.#socket !== socket) return;
			this.#socket = null;
			if (this.#disposed) return;
			this.#scheduleReconnect();
		};
	}

	#sendSubscribe(socket: RealtimeWebSocket): void {
		if (socket.readyState !== socketOpen || this.topics.length === 0) return;
		socket.send(JSON.stringify({ type: 'subscribe', topics: this.topics }));
	}

	#handleMessage(message: MessageEvent): void {
		if (typeof message.data !== 'string') return;
		let payload: unknown;
		try {
			payload = JSON.parse(message.data);
		} catch {
			return;
		}

		const invalidate = realtimeInvalidateEventSchema.safeParse(payload);
		if (invalidate.success) {
			if (!this.topics.includes(invalidate.data.topic)) return;
			this.#onInvalidate(invalidate.data);
			return;
		}

		const subscribed = realtimeSubscribedEventSchema.safeParse(payload);
		if (subscribed.success) {
			if (!this.#hasEveryTopic(subscribed.data.topics)) return;
			this.#retryAttempt = 0;
			this.#setConnectionState('connected');
			return;
		}

		const resyncRequired = realtimeResyncRequiredEventSchema.safeParse(payload);
		if (resyncRequired.success) {
			if (
				resyncRequired.data.topics &&
				!resyncRequired.data.topics.some((topic) => this.topics.includes(topic))
			) {
				return;
			}
			this.#onResyncRequired?.();
			return;
		}

		const protocolError = realtimeErrorEventSchema.safeParse(payload);
		if (!protocolError.success) return;
		this.#onProtocolError?.(protocolError.data);
		this.#setConnectionState('offline');
	}

	#hasEveryTopic(topics: readonly string[]): boolean {
		return this.topics.every((topic) => topics.includes(topic));
	}

	#scheduleReconnect(): void {
		if (this.#disposed || this.#retryTimer !== null) return;
		if (!this.#runtime.isOnline()) {
			this.#setConnectionState('offline');
			return;
		}

		this.#setConnectionState('reconnecting');
		const delay = this.#nextRetryDelay();
		this.#retryTimer = this.#runtime.setTimeout(() => {
			this.#retryTimer = null;
			this.#open();
		}, delay);
	}

	#nextRetryDelay(): number {
		const exponentialDelay = Math.min(
			this.#initialDelayMs * 2 ** this.#retryAttempt,
			this.#maxDelayMs
		);
		this.#retryAttempt += 1;
		const jitter = 0.8 + this.#runtime.random() * 0.4;
		return Math.round(exponentialDelay * jitter);
	}

	#clearRetryTimer(): void {
		if (this.#retryTimer === null) return;
		this.#runtime.clearTimeout(this.#retryTimer);
		this.#retryTimer = null;
	}

	#closeSocket(): void {
		const socket = this.#socket;
		this.#socket = null;
		if (!socket) return;
		socket.onopen = null;
		socket.onmessage = null;
		socket.onerror = null;
		socket.onclose = null;
		socket.close();
	}

	#setConnectionState(state: RealtimeConnectionState): void {
		if (this.#connectionState === state) return;
		this.#connectionState = state;
		this.#onConnectionStateChange?.(state);
	}
}

export function createBrowserRealtimeSocket(
	options: BrowserRealtimeSocketOptions
): BrowserRealtimeSocket {
	return new BrowserRealtimeSocket(options);
}
