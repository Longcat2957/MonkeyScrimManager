import { readFileSync } from 'node:fs';
import { render } from 'svelte/server';
import { describe, expect, it } from 'vitest';
import RecruitmentDummyParticipantPanel from './RecruitmentDummyParticipantPanel.svelte';

const source = readFileSync(
	new URL('./RecruitmentDummyParticipantPanel.svelte', import.meta.url),
	'utf8'
);

describe('RecruitmentDummyParticipantPanel', () => {
	it('renders as an explicitly development-only collapsed control', () => {
		const { body } = render(RecruitmentDummyParticipantPanel, {
			props: {
				guildId: '1525851181157580861',
				recruitmentId: '01900000-0000-7000-8000-000000000020'
			}
		});

		expect(body).toContain('더미 참가자');
		expect(body).toContain('개발 전용');
		expect(body).toContain('aria-expanded="false"');
	});

	it('loads additional guild users beyond the first 100 without replacing prior selections', () => {
		expect(source).toContain('userTotal = userPage.total');
		expect(source).toContain('offset: users.length');
		expect(source).toContain('users = [...users, ...page.users.filter');
		expect(source).toContain('{#if users.length < userTotal}');
		expect(source).toContain('더미 사용자 더 불러오기');
	});

	it('keeps one idempotency key for an unchanged participant operation', () => {
		expect(source).toContain('mutationKeys.keyFor(participantOperation, command)');
		expect(source).toContain('mutationKeys.complete(participantOperation)');
		expect(source).toContain('mutationKeys.invalidate(participantOperation)');
		expect(source).not.toContain('globalThis.crypto.randomUUID()');
	});
});
