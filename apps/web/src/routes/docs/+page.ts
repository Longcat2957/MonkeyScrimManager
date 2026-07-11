import { marked } from 'marked';
import guideMarkdown from '$lib/content/user-guide.md?raw';
import type { PageLoad } from './$types';

export const prerender = true;

export const load: PageLoad = () => {
	return {
		content: marked.parse(guideMarkdown, { async: false })
	};
};
