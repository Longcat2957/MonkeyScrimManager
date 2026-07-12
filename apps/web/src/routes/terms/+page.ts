import { marked } from 'marked';
import termsMarkdown from '$lib/content/terms.md?raw';
import type { PageLoad } from './$types';

export const prerender = true;

export const load: PageLoad = () => {
	return {
		content: marked.parse(termsMarkdown, { async: false })
	};
};
