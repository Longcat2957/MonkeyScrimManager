<script lang="ts">
	import AuctionTournamentBidDraftWorkbench from '$lib/components/workflow/AuctionTournamentBidDraftWorkbench.svelte';
	import type {
		AuctionTournamentCurrentTargetProfile,
		AuctionTournamentDraftCandidate,
		AuctionTournamentDraftCommand,
		AuctionTournamentDraftSnapshot,
		AuctionTournamentTeamMember
	} from '$lib/contracts/auctionTournamentDraft';

	const teamOneId = '019f8100-0000-7000-8000-000000000001';
	const teamTwoId = '019f8100-0000-7000-8000-000000000002';

	const candidates: AuctionTournamentDraftCandidate[] = [
		{
			userId: '310000000000000001',
			displayName: 'Raven',
			declaredLane: 'MID',
			mmrAtStart: 1623,
			snapshotOrigin: 'CAPTURED',
			queuePosition: 0,
			state: 'ACTIVE',
			drawCount: 1
		},
		{
			userId: '310000000000000002',
			displayName: 'Mango',
			declaredLane: 'JUNGLE',
			mmrAtStart: 1580,
			snapshotOrigin: 'CAPTURED',
			queuePosition: 1,
			state: 'QUEUED',
			drawCount: 0
		},
		{
			userId: '310000000000000003',
			displayName: 'Mori',
			declaredLane: 'TOP',
			mmrAtStart: 1548,
			snapshotOrigin: 'CAPTURED',
			queuePosition: 2,
			state: 'QUEUED',
			drawCount: 0
		},
		{
			userId: '310000000000000004',
			displayName: 'Nori',
			declaredLane: 'BOTTOM',
			mmrAtStart: 1596,
			snapshotOrigin: 'CAPTURED',
			queuePosition: 3,
			state: 'QUEUED',
			drawCount: 0
		},
		{
			userId: '310000000000000005',
			displayName: 'Haru',
			declaredLane: 'SUPPORT',
			mmrAtStart: 1512,
			snapshotOrigin: 'CAPTURED',
			queuePosition: 4,
			state: 'QUEUED',
			drawCount: 0
		},
		{
			userId: '310000000000000006',
			displayName: 'Mako',
			declaredLane: 'TOP',
			mmrAtStart: 1572,
			snapshotOrigin: 'CAPTURED',
			queuePosition: 5,
			state: 'QUEUED',
			drawCount: 0
		},
		{
			userId: '310000000000000007',
			displayName: 'Sumi',
			declaredLane: 'JUNGLE',
			mmrAtStart: 1604,
			snapshotOrigin: 'CAPTURED',
			queuePosition: 6,
			state: 'QUEUED',
			drawCount: 0
		},
		{
			userId: '310000000000000008',
			displayName: 'Kite',
			declaredLane: 'BOTTOM',
			mmrAtStart: 1508,
			snapshotOrigin: 'CAPTURED',
			queuePosition: 7,
			state: 'QUEUED',
			drawCount: 0
		}
	];

	function profileFor(
		candidate: AuctionTournamentDraftCandidate
	): AuctionTournamentCurrentTargetProfile {
		const mainLane = candidate.declaredLane ?? 'MID';
		const mmr = candidate.mmrAtStart ?? 1500;
		return {
			avatarUrl: null,
			profileIconId: null,
			riotTier: 'P2',
			mainLane,
			summary: { games: 12, wins: 7, losses: 5 },
			laneMmr: [
				{ lane: 'TOP', mmr: mainLane === 'TOP' ? mmr : 1500, games: 2, wins: 1 },
				{ lane: 'JUNGLE', mmr: mainLane === 'JUNGLE' ? mmr : 1500, games: 2, wins: 1 },
				{ lane: 'MID', mmr: mainLane === 'MID' ? mmr : 1500, games: 4, wins: 3 },
				{ lane: 'BOTTOM', mmr: mainLane === 'BOTTOM' ? mmr : 1500, games: 2, wins: 1 },
				{ lane: 'SUPPORT', mmr: mainLane === 'SUPPORT' ? mmr : 1500, games: 2, wins: 1 }
			],
			championIds: [],
			championRecords: [],
			recentGames: [],
			riot: null
		};
	}

	function memberFor(
		candidate: AuctionTournamentDraftCandidate,
		acquiredVia: AuctionTournamentTeamMember['acquiredVia'],
		rosterSlot: number
	): AuctionTournamentTeamMember {
		return {
			teamMemberId: `019f8100-0000-7000-8000-${String(candidate.queuePosition + 100).padStart(12, '0')}`,
			userId: candidate.userId,
			displayName: candidate.displayName,
			declaredLane: candidate.declaredLane,
			mmrAtAcquisition: candidate.mmrAtStart,
			snapshotOrigin: candidate.snapshotOrigin,
			acquiredVia,
			rosterSlot
		};
	}

	const initialSnapshot: AuctionTournamentDraftSnapshot = {
		tournament: {
			id: '019f8100-0000-7000-8000-000000000010',
			sourceRecruitmentId: '019f8100-0000-7000-8000-000000000011',
			displayNumber: 24,
			title: '토요 경매 내전',
			format: 'TEN_PERSON',
			rules: { hardFearless: false, semifinalBestOf: null, finalBestOf: 3 },
			status: 'BIDDING',
			revision: 0,
			href: '/g/demo/auction/tournaments/demo'
		},
		teams: [
			{
				id: teamOneId,
				teamIndex: 1,
				teamName: '1팀',
				initialPoints: 100,
				currentPoints: 100,
				pointRevision: 0,
				members: [
					{
						teamMemberId: '019f8100-0000-7000-8000-000000000090',
						userId: '310000000000000101',
						displayName: 'Captain One',
						declaredLane: 'TOP',
						mmrAtAcquisition: 1600,
						snapshotOrigin: 'CAPTURED',
						acquiredVia: 'CAPTAIN',
						rosterSlot: 1
					}
				]
			},
			{
				id: teamTwoId,
				teamIndex: 2,
				teamName: '2팀',
				initialPoints: 100,
				currentPoints: 100,
				pointRevision: 0,
				members: [
					{
						teamMemberId: '019f8100-0000-7000-8000-000000000091',
						userId: '310000000000000102',
						displayName: 'Captain Two',
						declaredLane: 'JUNGLE',
						mmrAtAcquisition: 1588,
						snapshotOrigin: 'CAPTURED',
						acquiredVia: 'CAPTAIN',
						rosterSlot: 1
					}
				]
			}
		],
		candidates,
		draft: {
			currentTarget: candidates[0]!,
			currentTargetProfile: profileFor(candidates[0]!),
			unsold: [],
			recentAction: null
		},
		permissions: { canManageDraft: true, canManageBracket: false, canRevertLast: false }
	};

	let snapshot = $state<AuctionTournamentDraftSnapshot>(initialSnapshot);

	function updateSnapshot(
		update: Pick<AuctionTournamentDraftSnapshot, 'teams' | 'candidates' | 'draft'>
	) {
		snapshot = {
			...snapshot,
			...update,
			tournament: { ...snapshot.tournament, revision: snapshot.tournament.revision + 1 }
		};
	}

	function assignCandidate(
		candidate: AuctionTournamentDraftCandidate,
		teamId: string,
		acquiredVia: 'BID' | 'MANUAL',
		action: 'AWARD_BID' | 'ASSIGN_CURRENT' | 'ASSIGN_UNSOLD',
		points = 0
	) {
		const selectedTeam = snapshot.teams.find((team) => team.id === teamId);
		if (!selectedTeam || selectedTeam.members.length >= 5) return;

		const teams = snapshot.teams.map((team) =>
			team.id !== teamId
				? team
				: {
						...team,
						currentPoints:
							acquiredVia === 'BID' ? Math.max(0, team.currentPoints - points) : team.currentPoints,
						pointRevision: acquiredVia === 'BID' ? team.pointRevision + 1 : team.pointRevision,
						members: [...team.members, memberFor(candidate, acquiredVia, team.members.length + 1)]
					}
		);
		const candidates = snapshot.candidates.map((entry) =>
			entry.userId === candidate.userId ? { ...entry, state: 'ASSIGNED' as const } : entry
		);
		updateSnapshot({
			teams,
			candidates,
			draft: {
				currentTarget: null,
				currentTargetProfile: null,
				unsold: snapshot.draft.unsold.filter((entry) => entry.userId !== candidate.userId),
				recentAction: { kind: action }
			}
		});
	}

	function handleCommand(command: AuctionTournamentDraftCommand) {
		const target = snapshot.draft.currentTarget;

		switch (command.kind) {
			case 'DRAW_NEXT': {
				if (target) return;
				const nextTarget = snapshot.candidates.find((candidate) => candidate.state === 'QUEUED');
				if (!nextTarget) return;
				const candidates = snapshot.candidates.map((candidate) =>
					candidate.userId === nextTarget.userId
						? { ...candidate, state: 'ACTIVE' as const, drawCount: candidate.drawCount + 1 }
						: candidate
				);
				const activeTarget = candidates.find(
					(candidate) => candidate.userId === nextTarget.userId
				)!;
				updateSnapshot({
					teams: snapshot.teams,
					candidates,
					draft: {
						currentTarget: activeTarget,
						currentTargetProfile: profileFor(activeTarget),
						unsold: snapshot.draft.unsold,
						recentAction: { kind: 'DRAW_NEXT' }
					}
				});
				return;
			}
			case 'MARK_UNSOLD': {
				if (!target || command.memberId !== target.userId) return;
				const unsoldTarget = { ...target, state: 'UNSOLD' as const };
				updateSnapshot({
					teams: snapshot.teams,
					candidates: snapshot.candidates.map((candidate) =>
						candidate.userId === target.userId ? unsoldTarget : candidate
					),
					draft: {
						currentTarget: null,
						currentTargetProfile: null,
						unsold: [...snapshot.draft.unsold, unsoldTarget],
						recentAction: { kind: 'MARK_UNSOLD' }
					}
				});
				return;
			}
			case 'AWARD_BID':
				if (target && command.memberId === target.userId) {
					assignCandidate(target, command.teamId, 'BID', 'AWARD_BID', command.points);
				}
				return;
			case 'ASSIGN_CURRENT':
				if (target && command.memberId === target.userId) {
					assignCandidate(target, command.teamId, 'MANUAL', 'ASSIGN_CURRENT');
				}
				return;
			case 'ASSIGN_UNSOLD': {
				const unsold = snapshot.draft.unsold.find(
					(candidate) => candidate.userId === command.memberId
				);
				if (unsold) assignCandidate(unsold, command.teamId, 'MANUAL', 'ASSIGN_UNSOLD');
				return;
			}
			case 'RESTORE_UNSOLD': {
				if (target) return;
				const unsold = snapshot.draft.unsold.find(
					(candidate) => candidate.userId === command.memberId
				);
				if (!unsold) return;
				const activeTarget = { ...unsold, state: 'ACTIVE' as const };
				updateSnapshot({
					teams: snapshot.teams,
					candidates: snapshot.candidates.map((candidate) =>
						candidate.userId === activeTarget.userId ? activeTarget : candidate
					),
					draft: {
						currentTarget: activeTarget,
						currentTargetProfile: profileFor(activeTarget),
						unsold: snapshot.draft.unsold.filter(
							(candidate) => candidate.userId !== activeTarget.userId
						),
						recentAction: { kind: 'RESTORE_UNSOLD' }
					}
				});
				return;
			}
			case 'REVERT_LAST':
				return;
		}
	}
</script>

<AuctionTournamentBidDraftWorkbench {snapshot} onCommand={handleCommand} />
