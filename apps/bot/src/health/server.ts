import { createServer, type Server, type ServerResponse } from 'node:http';

import type { Logger } from 'pino';

export interface DependencyReadiness {
	ready: boolean;
	checkedAt: string | null;
}

export interface ReadinessState {
	discord: DependencyReadiness;
	api: DependencyReadiness;
	shuttingDown?: boolean;
}

export interface HealthServerOptions {
	host: string;
	port: number;
	getReadiness: () => ReadinessState;
	logger?: Logger;
}

export interface HealthServerHandle {
	host: string;
	port: number;
	close: () => Promise<void>;
}

export async function startHealthServer(options: HealthServerOptions): Promise<HealthServerHandle> {
	if (!options.host.trim()) throw new Error('health host is required');
	if (!Number.isInteger(options.port) || options.port < 1 || options.port > 65_535) {
		throw new Error('health port must be an integer between 1 and 65535');
	}

	const server = createServer((request, response) => {
		response.setHeader('cache-control', 'no-store');
		response.setHeader('content-type', 'application/json; charset=utf-8');
		response.setHeader('x-content-type-options', 'nosniff');

		if (request.method !== 'GET' && request.method !== 'HEAD') {
			response.setHeader('allow', 'GET, HEAD');
			writeJson(response, request.method === 'HEAD', 405, { error: 'method_not_allowed' });
			return;
		}

		const pathname = new URL(request.url ?? '/', 'http://health.local').pathname;
		if (pathname === '/healthz') {
			writeJson(response, request.method === 'HEAD', 200, { status: 'ok' });
			return;
		}
		if (pathname !== '/readyz') {
			writeJson(response, request.method === 'HEAD', 404, { error: 'not_found' });
			return;
		}

		let readiness: ReadinessState;
		try {
			readiness = options.getReadiness();
		} catch (error) {
			options.logger?.error({ err: error }, 'bot readiness provider failed');
			writeJson(response, request.method === 'HEAD', 503, {
				status: 'not_ready',
				acceptingRequests: false,
				dependencies: {
					discord: { ready: false, checkedAt: null },
					api: { ready: false, checkedAt: null }
				}
			});
			return;
		}

		const ready = readiness.discord.ready && readiness.api.ready && !readiness.shuttingDown;
		writeJson(response, request.method === 'HEAD', ready ? 200 : 503, {
			status: ready ? 'ready' : 'not_ready',
			acceptingRequests: ready,
			dependencies: {
				discord: readiness.discord,
				api: readiness.api
			}
		});
	});

	server.on('clientError', (error, socket) => {
		options.logger?.warn({ err: error }, 'bot health client error');
		socket.end('HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n');
	});

	await listen(server, options.host, options.port);
	options.logger?.info({ host: options.host, port: options.port }, 'bot health server listening');

	let closed = false;
	return {
		host: options.host,
		port: options.port,
		close: async () => {
			if (closed) return;
			closed = true;
			await close(server);
			options.logger?.info('bot health server closed');
		}
	};
}

function writeJson(
	response: ServerResponse,
	headOnly: boolean,
	status: number,
	payload: unknown
): void {
	const body = JSON.stringify(payload);
	response.statusCode = status;
	response.setHeader('content-length', Buffer.byteLength(body));
	response.end(headOnly ? undefined : body);
}

function listen(server: Server, host: string, port: number): Promise<void> {
	return new Promise((resolve, reject) => {
		const onError = (error: Error) => {
			server.off('listening', onListening);
			reject(error);
		};
		const onListening = () => {
			server.off('error', onError);
			resolve();
		};
		server.once('error', onError);
		server.once('listening', onListening);
		server.listen(port, host);
	});
}

function close(server: Server): Promise<void> {
	return new Promise((resolve, reject) => {
		server.close((error) => {
			if (error) reject(error);
			else resolve();
		});
	});
}
