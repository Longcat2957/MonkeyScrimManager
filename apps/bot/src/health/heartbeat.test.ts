import { EventEmitter } from 'node:events';

import { Events, type Client } from 'discord.js';
import type { Logger } from 'pino';
import { afterEach, describe, expect, it, vi } from 'vitest';

import type { MonkeyBotApiClient } from '../api/client.js';
import { installBotHeartbeatReporter } from './heartbeat.js';

describe('BotHeartbeatReporter', () => {
	afterEach(() => {
		vi.useRealTimers();
	});

	it('starts after Discord is ready, reports immediately, repeats, and stops cleanly', async () => {
		vi.useFakeTimers();
		const client = new EventEmitter();
		const heartbeat = vi.fn(async () => ({
			accepted: true as const,
			lastHeartbeatAt: '2026-07-15T12:00:00Z'
		}));
		const warn = vi.fn();
		const reporter = installBotHeartbeatReporter(
			client as unknown as Client,
			{ heartbeat } as Pick<MonkeyBotApiClient, 'heartbeat'>,
			{ warn } as unknown as Logger,
			{
				instanceId: 'bot-main-1',
				version: '0.1.0',
				commitSha: 'abc1234',
				intervalMs: 30_000
			}
		);

		await vi.advanceTimersByTimeAsync(30_000);
		expect(heartbeat).not.toHaveBeenCalled();

		client.emit(Events.ClientReady, client);
		await vi.advanceTimersByTimeAsync(0);
		expect(heartbeat).toHaveBeenCalledTimes(1);
		expect(heartbeat).toHaveBeenLastCalledWith(
			{ instanceId: 'bot-main-1', version: '0.1.0', commitSha: 'abc1234' },
			expect.objectContaining({ signal: expect.any(AbortSignal) })
		);

		await vi.advanceTimersByTimeAsync(30_000);
		expect(heartbeat).toHaveBeenCalledTimes(2);

		reporter.close();
		await vi.advanceTimersByTimeAsync(60_000);
		expect(heartbeat).toHaveBeenCalledTimes(2);
		expect(warn).not.toHaveBeenCalled();
	});

	it('records a safe warning when the API rejects a heartbeat', async () => {
		vi.useFakeTimers();
		const client = new EventEmitter();
		const heartbeat = vi.fn(async () => {
			throw new Error('database password must not be exposed');
		});
		const warn = vi.fn();
		const reporter = installBotHeartbeatReporter(
			client as unknown as Client,
			{ heartbeat } as Pick<MonkeyBotApiClient, 'heartbeat'>,
			{ warn } as unknown as Logger,
			{
				instanceId: 'bot-main-1',
				version: '0.1.0',
				commitSha: null,
				intervalMs: 30_000
			}
		);

		client.emit(Events.ClientReady, client);
		await vi.advanceTimersByTimeAsync(0);
		expect(warn).toHaveBeenCalledWith(
			expect.objectContaining({ instanceId: 'bot-main-1' }),
			'Bot heartbeat report failed'
		);
		reporter.close();
	});
});
