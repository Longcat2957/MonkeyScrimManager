export type RealtimeConnectionState = 'connected' | 'reconnecting' | 'offline';
export type RealtimeSaveState = 'saved' | 'saving' | 'dirty' | 'conflict';

export interface RealtimeEditor {
	id: string;
	name: string;
	isSelf?: boolean;
}
