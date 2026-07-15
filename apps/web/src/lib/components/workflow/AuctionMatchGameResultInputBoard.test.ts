import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const source = readFileSync(
	new URL('./AuctionMatchGameResultInputBoard.svelte', import.meta.url),
	'utf8'
);
const normalizedSource = source.replace(/\s+/g, ' ');

describe('AuctionMatchGameResultInputBoard', () => {
	it('adapts the shared series pick-ban workspace to the auction game command', () => {
		expect(source).toContain(
			"import ScrimPickBanResultInputBoard from './ScrimPickBanResultInputBoard.svelte'"
		);
		expect(source).toContain(
			"import ScrimPickBanResultSetSummaryCard from './ScrimPickBanResultSetSummaryCard.svelte'"
		);
		expect(source).toContain('<ScrimPickBanResultInputBoard');
		expect(source).toContain('teams={boardTeams}');
		expect(source).toContain('hardFearless={snapshot.match.hardFearless}');
		expect(source).toContain('validateDraft');
		expect(source).toContain('allowLocalBulkInput');
		expect(source).toContain('expectedRevision: snapshot.match.revision');
		expect(source).toContain('setNumber: nextSetNumber');
		expect(source).toContain('patchVersion: catalog.version');
	});

	it('renders the selected completed auction game with the shared detailed pick-ban result card', () => {
		expect(source).toContain('const confirmedGames = $derived(');
		expect(source).toContain('let selectedCompletedGameId = $state<string | null>(null)');
		expect(source).toContain('const completedSetNumbers = $derived(');
		expect(source).toContain('const selectedCompletedGame = $derived(');
		expect(source).toContain('function teamsForConfirmedGame(game: AuctionMatchConfirmedGame)');
		expect(source).toContain("game.sidePreset === 'DEFAULT' ? 'BLUE' : 'RED'");
		expect(source).toContain("snapshot.match.status === 'COMPLETED'");
		expect(source).toContain('<ScrimPickBanResultSetSummaryCard');
		expect(source).toContain('selectedCompletedGame.id === game.id');
		expect(source).toContain('blueTeam={gameTeams.BLUE}');
		expect(source).toContain('redTeam={gameTeams.RED}');
		expect(source).toContain('bans={selectedCompletedGame.bans}');
		expect(source).toContain('picks={selectedCompletedGame.picks}');
		expect(source).toContain('version={selectedCompletedGame.patchVersion ?? catalog.version}');
	});

	it('requires explicit confirmation before reverting a confirmed game result', () => {
		expect(source).toContain('let revertDialogOpen = $state(false)');
		expect(source).toContain('onclick={() => (revertDialogOpen = true)}');
		expect(normalizedSource).toMatch(
			/<Dialog\.Title[^>]*>\s*마지막 게임 결과 되돌리기<\/Dialog\.Title\s*>/
		);
		expect(source).toContain('onclick={() => void deleteLastGame()}');
		expect(source).toContain('revertDialogOpen = false;');
	});

	it('explains score, MMR, and completed-final Discord feedback effects before confirmation', () => {
		expect(normalizedSource).toContain('경기 점수, 이미 반영된 길드 MMR을 함께 되돌립니다.');
		expect(normalizedSource).toContain(
			'완료된 결승의 Discord 결과 메시지는 서버가 전송 상태를 확인해 정리하며'
		);
		expect(normalizedSource).toContain('확인할 수 없으면 이 작업은 진행되지 않습니다.');
	});
});
