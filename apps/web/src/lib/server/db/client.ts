import type { DrizzleD1Database } from 'drizzle-orm/d1';
import * as schema from './schema';

export type AppDatabase = DrizzleD1Database<typeof schema>;

export function getDb(): AppDatabase {
	throw new Error('D1 Drizzle client is not wired during the frontend scaffold phase.');
}
