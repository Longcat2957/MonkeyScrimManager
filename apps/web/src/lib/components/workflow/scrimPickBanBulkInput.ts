import type { SeriesPickBanBulkItem } from '$lib/contracts/seriesPickBan';
import type {
	ScrimPickBanResultInputChampion,
	ScrimPickBanResultInputSide,
	ScrimPickBanResultInputTeam
} from '$lib/fixtures/scrimPickBanResultInputFixtures';
import { normalizeScrimPickBanResultInputChampionName } from './scrimPickBanResultInputHelpers';

export const scrimPickBanBulkInputFieldKeys = [
	'BLUE_PICKS',
	'RED_PICKS',
	'BLUE_BANS',
	'RED_BANS'
] as const;

export type ScrimPickBanBulkInputFieldKey = (typeof scrimPickBanBulkInputFieldKeys)[number];

export type ScrimPickBanBulkInputValues = Record<ScrimPickBanBulkInputFieldKey, string>;

export interface ScrimPickBanBulkInputFieldDefinition {
	key: ScrimPickBanBulkInputFieldKey;
	label: string;
	placeholder: string;
	kind: 'PICK' | 'BAN';
	side: ScrimPickBanResultInputSide;
}

export interface ScrimPickBanBulkInputFeedback {
	severity: 'error' | 'notice';
	message: string;
}

export interface ScrimPickBanBulkInputFieldPlan {
	definition: ScrimPickBanBulkInputFieldDefinition;
	feedback: ScrimPickBanBulkInputFeedback[];
	operations: SeriesPickBanBulkItem[];
}

export interface ScrimPickBanBulkInputPlan {
	fields: ScrimPickBanBulkInputFieldPlan[];
	operations: SeriesPickBanBulkItem[];
	hasValidationErrors: boolean;
	canApply: boolean;
}

interface ScrimPickBanBulkInputCandidate {
	fieldKey: ScrimPickBanBulkInputFieldKey;
	tokenIndex: number;
	slotKey: string;
	championId: string;
	operation: SeriesPickBanBulkItem | null;
}

interface ScrimPickBanBulkInputBuildOptions {
	values: ScrimPickBanBulkInputValues;
	teams: Record<ScrimPickBanResultInputSide, ScrimPickBanResultInputTeam>;
	picks: Record<string, string | null>;
	bans: Record<ScrimPickBanResultInputSide, Array<string | null>>;
	champions: ScrimPickBanResultInputChampion[];
	hardFearless?: boolean;
	previousPickChampionIds?: readonly string[];
}

const laneOrder = ['TOP', 'JUNGLE', 'MID', 'BOTTOM', 'SUPPORT'] as const;

export const scrimPickBanBulkInputFields: readonly ScrimPickBanBulkInputFieldDefinition[] = [
	{
		key: 'BLUE_PICKS',
		label: 'BLUE 픽',
		placeholder: '예: 나르, 비에고, 아리, 이즈리얼, 쓰레쉬',
		kind: 'PICK',
		side: 'BLUE'
	},
	{
		key: 'RED_PICKS',
		label: 'RED 픽',
		placeholder: '예: 뽀삐, 바이, 신드라, 카이사, 알리스타',
		kind: 'PICK',
		side: 'RED'
	},
	{
		key: 'BLUE_BANS',
		label: 'BLUE 밴',
		placeholder: '예: 아트록스, 세주아니, 아지르, 케이틀린, 라칸',
		kind: 'BAN',
		side: 'BLUE'
	},
	{
		key: 'RED_BANS',
		label: 'RED 밴',
		placeholder: '예: 나르, 리신, 오리아나, 징크스, 노틸러스',
		kind: 'BAN',
		side: 'RED'
	}
];

export const emptyScrimPickBanBulkInputValues = (): ScrimPickBanBulkInputValues => ({
	BLUE_PICKS: '',
	RED_PICKS: '',
	BLUE_BANS: '',
	RED_BANS: ''
});

interface ChampionMatch {
	champion: ScrimPickBanResultInputChampion | null;
	error: string | null;
}

export function matchScrimPickBanBulkInputChampion(
	token: string,
	champions: ScrimPickBanResultInputChampion[]
): ChampionMatch {
	const query = normalizeScrimPickBanResultInputChampionName(token);
	if (!query) return { champion: null, error: null };

	const exact = uniqueChampionMatches(
		champions.filter((champion) => championTokens(champion).some((value) => value === query))
	);
	if (exact.length === 1) return { champion: exact[0]!, error: null };
	if (exact.length > 1) return ambiguousChampionMatch(token, exact);

	const prefix = uniqueChampionMatches(
		champions.filter((champion) => championTokens(champion).some((value) => value.startsWith(query)))
	);
	if (prefix.length === 1) return { champion: prefix[0]!, error: null };
	if (prefix.length > 1) return ambiguousChampionMatch(token, prefix);

	const includes = uniqueChampionMatches(
		champions.filter((champion) => championTokens(champion).some((value) => value.includes(query)))
	);
	if (includes.length === 1) return { champion: includes[0]!, error: null };
	if (includes.length > 1) return ambiguousChampionMatch(token, includes);

	return { champion: null, error: `“${token}”에 해당하는 챔피언을 찾을 수 없습니다.` };
}

export function buildScrimPickBanBulkInputPlan(
	options: ScrimPickBanBulkInputBuildOptions
): ScrimPickBanBulkInputPlan {
	const feedbackByField = new Map<ScrimPickBanBulkInputFieldKey, ScrimPickBanBulkInputFeedback[]>();
	const candidates: ScrimPickBanBulkInputCandidate[] = [];
	const currentSlots = currentChampionSlots(options.picks, options.bans);

	for (const definition of scrimPickBanBulkInputFields) {
		feedbackByField.set(definition.key, []);
		const input = options.values[definition.key];
		if (!input.trim()) continue;

		const tokens = input.split(',');
		if (tokens.length > 5) {
			feedbackByField.get(definition.key)?.push({
				severity: 'error',
				message: '최대 5개까지 쉼표로 구분해 입력할 수 있습니다.'
			});
			continue;
		}

		const slotTargets = targetsForDefinition(definition, options.teams);
		if (!slotTargets) {
			feedbackByField.get(definition.key)?.push({
				severity: 'error',
				message: '팀 라인업이 완전하지 않아 일괄 입력을 적용할 수 없습니다.'
			});
			continue;
		}

		for (const [tokenIndex, rawToken] of tokens.entries()) {
			const token = rawToken.trim();
			if (!token) {
				feedbackByField.get(definition.key)?.push({
					severity: 'notice',
					message: `${tokenIndex + 1}번째 빈 항목은 기존 값을 유지합니다.`
				});
				continue;
			}

			const match = matchScrimPickBanBulkInputChampion(token, options.champions);
			if (!match.champion) {
				feedbackByField.get(definition.key)?.push({
					severity: 'error',
					message: match.error ?? `“${token}”을(를) 확인할 수 없습니다.`
				});
				continue;
			}

			const target = slotTargets[tokenIndex];
			if (!target) continue;
			const operation = operationForTarget(target, match.champion.id, currentSlots);
			candidates.push({
				fieldKey: definition.key,
				tokenIndex,
				slotKey: target.slotKey,
				championId: match.champion.id,
				operation
			});
		}
	}

	validateFinalChampionState({
		candidates,
		currentSlots,
		feedbackByField,
		hardFearless: options.hardFearless ?? false,
		previousPickChampionIds: options.previousPickChampionIds ?? []
	});

	const fields = scrimPickBanBulkInputFields.map((definition) => ({
		definition,
		feedback: feedbackByField.get(definition.key) ?? [],
		operations: candidates
			.filter((candidate) => candidate.fieldKey === definition.key && candidate.operation)
			.map((candidate) => candidate.operation!)
	}));
	const operations = candidates
		.map((candidate) => candidate.operation)
	.filter((operation): operation is SeriesPickBanBulkItem => operation !== null);
	const hasValidationErrors = fields.some((field) =>
		field.feedback.some((feedback) => feedback.severity === 'error')
	);

	return {
		fields,
		operations,
		hasValidationErrors,
		canApply: !hasValidationErrors && operations.length > 0
	};
}

function championTokens(champion: ScrimPickBanResultInputChampion): string[] {
	return [
		normalizeScrimPickBanResultInputChampionName(champion.name),
		normalizeScrimPickBanResultInputChampionName(champion.id)
	].filter(Boolean);
}

function uniqueChampionMatches(
	champions: ScrimPickBanResultInputChampion[]
): ScrimPickBanResultInputChampion[] {
	return Array.from(new Map(champions.map((champion) => [champion.id, champion])).values());
}

function ambiguousChampionMatch(
	token: string,
	matches: ScrimPickBanResultInputChampion[]
): ChampionMatch {
	const names = matches
		.slice(0, 3)
		.map((champion) => champion.name)
		.join(', ');
	const suffix = matches.length > 3 ? ' 등' : '';
	return {
		champion: null,
		error: `“${token}”은(는) 여러 챔피언과 일치합니다: ${names}${suffix}. 전체 이름을 입력하세요.`
	};
}

interface SlotTarget {
	slotKey: string;
	kind: 'PICK' | 'BAN';
	playerId?: string;
	side?: ScrimPickBanResultInputSide;
	slot?: number;
}

function targetsForDefinition(
	definition: ScrimPickBanBulkInputFieldDefinition,
	teams: Record<ScrimPickBanResultInputSide, ScrimPickBanResultInputTeam>
): SlotTarget[] | null {
	if (definition.kind === 'BAN') {
		return Array.from({ length: 5 }, (_, index) => ({
			slotKey: banSlotKey(definition.side, index + 1),
			kind: 'BAN' as const,
			side: definition.side,
			slot: index + 1
		}));
	}

	const playersByLane = new Map(teams[definition.side].players.map((player) => [player.lane, player]));
	const targets = laneOrder.map((lane) => playersByLane.get(lane));
	if (targets.some((player) => !player)) return null;

	return targets.map((player) => ({
		slotKey: pickSlotKey(player!.id),
		kind: 'PICK' as const,
		playerId: player!.id
	}));
}

function currentChampionSlots(
	picks: Record<string, string | null>,
	bans: Record<ScrimPickBanResultInputSide, Array<string | null>>
): Map<string, string | null> {
	const slots = new Map<string, string | null>();
	for (const [playerId, championId] of Object.entries(picks)) {
		slots.set(pickSlotKey(playerId), championId);
	}
	for (const side of ['BLUE', 'RED'] as const) {
		for (const [index, championId] of bans[side].entries()) {
			slots.set(banSlotKey(side, index + 1), championId);
		}
	}
	return slots;
}

function operationForTarget(
	target: SlotTarget,
	championId: string,
	currentSlots: Map<string, string | null>
): SeriesPickBanBulkItem | null {
	if (currentSlots.get(target.slotKey) === championId) return null;
	if (target.kind === 'PICK' && target.playerId) {
		return { type: 'SET_PICK', playerId: target.playerId, championId };
	}
	if (target.kind === 'BAN' && target.side && target.slot) {
		return { type: 'SET_BAN', side: target.side, slot: target.slot, championId };
	}
	return null;
}

function validateFinalChampionState({
	candidates,
	currentSlots,
	feedbackByField,
	hardFearless,
	previousPickChampionIds
}: {
	candidates: ScrimPickBanBulkInputCandidate[];
	currentSlots: Map<string, string | null>;
	feedbackByField: Map<ScrimPickBanBulkInputFieldKey, ScrimPickBanBulkInputFeedback[]>;
	hardFearless: boolean;
	previousPickChampionIds: readonly string[];
}) {
	const finalSlots = new Map(currentSlots);
	for (const candidate of candidates) {
		finalSlots.set(candidate.slotKey, candidate.championId);
	}

	const candidatesBySlot = new Map(candidates.map((candidate) => [candidate.slotKey, candidate]));
	const slotsByChampion = new Map<string, string[]>();
	for (const [slotKey, championId] of finalSlots) {
		if (!championId) continue;
		const slots = slotsByChampion.get(championId) ?? [];
		slots.push(slotKey);
		slotsByChampion.set(championId, slots);
	}

	for (const [championId, slots] of slotsByChampion) {
		if (slots.length < 2) continue;
		for (const slotKey of slots) {
			const candidate = candidatesBySlot.get(slotKey);
			if (!candidate?.operation) continue;
			addCandidateError(
				feedbackByField,
				candidate,
				`“${championId}”은(는) 이 세트의 다른 픽 또는 밴과 중복됩니다.`
			);
		}
	}

	if (!hardFearless) return;
	const previous = new Set(previousPickChampionIds.map(normalizeScrimPickBanResultInputChampionName));
	for (const candidate of candidates) {
		if (!candidate.operation || !previous.has(normalizeScrimPickBanResultInputChampionName(candidate.championId))) {
			continue;
		}
		addCandidateError(
			feedbackByField,
			candidate,
			`“${candidate.championId}”은(는) 이전 세트에서 선택되어 Hard Fearless 규칙상 사용할 수 없습니다.`
		);
	}
}

function addCandidateError(
	feedbackByField: Map<ScrimPickBanBulkInputFieldKey, ScrimPickBanBulkInputFeedback[]>,
	candidate: ScrimPickBanBulkInputCandidate,
	message: string
) {
	const feedback = feedbackByField.get(candidate.fieldKey);
	if (!feedback?.some((item) => item.severity === 'error' && item.message === message)) {
		feedback?.push({ severity: 'error', message: `${candidate.tokenIndex + 1}번째 항목: ${message}` });
	}
}

function pickSlotKey(playerId: string): string {
	return `PICK:${playerId}`;
}

function banSlotKey(side: ScrimPickBanResultInputSide, slot: number): string {
	return `BAN:${side}:${slot}`;
}
