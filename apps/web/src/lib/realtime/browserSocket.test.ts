import { afterEach, describe, expect, it, vi } from 'vitest';
import {
	createBrowserRealtimeSocket,
	sameOriginWebSocketUrl,
	type RealtimeWebSocket
} from './browserSocket';

const recruitmentTopic =
	'guild:1525851181157580861:recruitment:01900000-0000-7000-8000-000000000101';
const draftTopic =
	'guild:1525851181157580861:recruitment-entry-draft:01900000-0000-7000-8000-000000000101';

class FakeSocket implements RealtimeWebSocket {
	readyState = 0;
	onopen: ((event: Event) => void) | null = null;
	onclose: ((event: CloseEvent) => void) | null = null;
	onerror: ((event: Event) => void) | null = null;
	onmessage: ((event: MessageEvent) => void) | null = null;
	sent: string[] = [];

	send(data: string): void {
		this.sent.push(data);
	}

	close(): void {
		this.readyState = 3;
		this.onclose?.({} as CloseEvent);
	}

	open(): void {
		this.readyState = 1;
		this.onopen?.(new Event('open'));
	}

	receive(payload: unknown): void {
		this.onmessage?.({ data: JSON.stringify(payload) } as MessageEvent);
	}
}

function createClient(
	overrides: {
		onInvalidate?: (event: { topic: string; revision: number }) => void;
		onResyncRequired?: () => void;
		onConnectionStateChange?: (state: string) => void;
	} = {}
) {
	const sockets: FakeSocket[] = [];
	const client = createBrowserRealtimeSocket({
		topics: [recruitmentTopic, draftTopic],
		url: 'ws://localhost:5173/ws',
		onInvalidate: (event) => overrides.onInvalidate?.(event),
		onResyncRequired: overrides.onResyncRequired,
		onConnectionStateChange: overrides.onConnectionStateChange,
		retry: { initialDelayMs: 100, maxDelayMs: 1_000 },
		runtime: {
			createWebSocket: () => {
				const socket = new FakeSocket();
				sockets.push(socket);
				return socket;
			},
			isOnline: () => true,
			random: () => 0.5,
			addOnlineListener: () => undefined,
			removeOnlineListener: () => undefined,
			addOfflineListener: () => undefined,
			removeOfflineListener: () => undefined
		}
	});
	return { client, sockets };
}

describe('BrowserRealtimeSocket', () => {
	afterEach(() => {
		vi.useRealTimers();
	});

	it('uses the same-origin websocket endpoint with the matching secure protocol', () => {
		expect(
			sameOriginWebSocketUrl({ origin: 'https://dev.monkeybot.lol', protocol: 'https:' })
		).toBe('wss://dev.monkeybot.lol/ws');
		expect(sameOriginWebSocketUrl({ origin: 'http://localhost:5173', protocol: 'http:' })).toBe(
			'ws://localhost:5173/ws'
		);
	});

	it('waits for both subscription acknowledgements before reporting a live connection', () => {
		const states: string[] = [];
		const { client, sockets } = createClient({
			onConnectionStateChange: (state) => states.push(state)
		});

		client.connect();
		const socket = sockets[0]!;
		socket.open();

		expect(JSON.parse(socket.sent[0]!)).toEqual({
			type: 'subscribe',
			topics: [recruitmentTopic, draftTopic]
		});
		expect(client.connectionState).toBe('reconnecting');

		socket.receive({ type: 'subscribed', topics: [recruitmentTopic] });
		expect(client.connectionState).toBe('reconnecting');

		socket.receive({ type: 'subscribed', topics: [recruitmentTopic, draftTopic] });
		expect(client.connectionState).toBe('connected');
		expect(states).toEqual(['reconnecting', 'connected']);
	});

	it('only forwards valid invalidate events for subscribed topics and accepts resync requests', () => {
		const invalidations: Array<{ topic: string; revision: number }> = [];
		const onResyncRequired = vi.fn();
		const { client, sockets } = createClient({
			onInvalidate: (event) => invalidations.push(event),
			onResyncRequired
		});
		client.connect();
		const socket = sockets[0]!;
		socket.open();

		socket.receive({
			type: 'invalidate',
			eventId: '01900000-0000-7000-8000-000000000001',
			topic: draftTopic,
			resourceId: '01900000-0000-7000-8000-000000000101',
			revision: 4,
			occurredAt: '2026-07-14T12:00:00Z'
		});
		socket.receive({
			type: 'invalidate',
			eventId: '01900000-0000-7000-8000-000000000002',
			topic: 'guild:other:recruitment:01900000-0000-7000-8000-000000000101',
			resourceId: '01900000-0000-7000-8000-000000000101',
			revision: 5,
			occurredAt: '2026-07-14T12:00:01Z'
		});
		socket.receive({ type: 'resync_required', topics: [draftTopic] });
		socket.receive({ type: 'resync_required', topics: ['guild:other:recruitment:other'] });
		socket.receive({ type: 'error', code: 'forbidden_topic', message: '권한이 없습니다.' });

		expect(invalidations.map(({ topic, revision }) => ({ topic, revision }))).toEqual([
			{ topic: draftTopic, revision: 4 }
		]);
		expect(onResyncRequired).toHaveBeenCalledTimes(1);
		expect(client.connectionState).toBe('offline');
	});

	it('reconnects with backoff after a close and stops reconnecting after disposal', () => {
		vi.useFakeTimers();
		const states: string[] = [];
		const { client, sockets } = createClient({
			onConnectionStateChange: (state) => states.push(state)
		});
		client.connect();
		sockets[0]!.open();
		sockets[0]!.receive({ type: 'subscribed', topics: [recruitmentTopic, draftTopic] });
		sockets[0]!.close();

		expect(client.connectionState).toBe('reconnecting');
		vi.advanceTimersByTime(100);
		expect(sockets).toHaveLength(2);

		client.disconnect();
		vi.advanceTimersByTime(1_000);
		expect(sockets).toHaveLength(2);
		expect(states).toEqual(['reconnecting', 'connected', 'reconnecting']);
	});
});
