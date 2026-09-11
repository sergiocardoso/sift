// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

// No `site`/`base` set on purpose: the docs aren't deployed anywhere yet
// and shouldn't assume a host (GitHub Pages, a custom domain, ...) ahead
// of that decision.
export default defineConfig({
	integrations: [
		starlight({
			title: 'Sift',
			description:
				'Safe, local-first file organization. Preview changes, apply explicitly, undo safely.',
			logo: {
				src: './src/assets/logo.png',
				alt: 'Sift',
				replacesTitle: true,
			},
			favicon: '/favicon.png',
			social: [
				{ icon: 'github', label: 'GitHub', href: 'https://github.com/sergiocardoso/sift' },
			],
			editLink: {
				baseUrl: 'https://github.com/sergiocardoso/sift/edit/main/website/',
			},
			customCss: ['./src/styles/custom.css'],
			// Monolingual for now (English only), but declared as an explicit
			// `root` locale rather than left implicit — adding `pt-BR` later
			// only means adding a sibling locale entry here and moving pages
			// under `src/content/docs/pt-br/`, never restructuring what
			// already exists.
			defaultLocale: 'root',
			locales: {
				root: { label: 'English', lang: 'en' },
			},
			sidebar: [
				{ label: 'Home', link: '/' },
				{
					label: 'Getting Started',
					items: [
						{ label: 'Introduction', slug: 'getting-started/introduction' },
						{ label: 'Installation', slug: 'getting-started/installation' },
						{ label: 'Quick Start', slug: 'getting-started/quick-start' },
						{ label: 'Your First Organize', slug: 'getting-started/first-organize' },
					],
				},
				{
					label: 'Core Concepts',
					items: [
						{ label: 'Safety Model', slug: 'core-concepts/safety-model' },
						{ label: 'Dry-run and --apply', slug: 'core-concepts/dry-run-and-apply' },
						{ label: 'Classification', slug: 'core-concepts/classification' },
						{ label: 'Protected Paths', slug: 'core-concepts/protected-paths' },
						{ label: 'History and Undo', slug: 'core-concepts/history-and-undo' },
						{ label: 'Explainability', slug: 'core-concepts/explainability' },
					],
				},
				{
					label: 'Organizing',
					items: [
						{ label: 'Organize Files', slug: 'organizing/organize-files' },
						{ label: 'Organize Folders', slug: 'organizing/organize-folders' },
						{ label: 'Recursive Organization', slug: 'organizing/recursive-organization' },
						{ label: 'Clean', slug: 'organizing/clean' },
						{ label: 'Doctor', slug: 'organizing/doctor' },
						{ label: 'Explain', slug: 'organizing/explain' },
						{ label: 'JSON and Scripting', slug: 'organizing/json-and-scripting' },
					],
				},
				{
					label: 'Configuration',
					items: [
						{ label: 'Overview', slug: 'configuration/overview' },
						{ label: '.sift.toml', slug: 'configuration/sift-toml' },
						{ label: 'Rules', slug: 'configuration/rules' },
						{ label: 'Rule Priority', slug: 'configuration/rule-priority' },
						{ label: 'Configuration Lookup', slug: 'configuration/configuration-lookup' },
					],
				},
				{
					label: 'Strategies',
					items: [
						{ label: 'Type', slug: 'strategies/type' },
						{ label: 'Date', slug: 'strategies/date' },
						{ label: 'Audio', slug: 'strategies/audio' },
						{ label: 'Video', slug: 'strategies/video' },
						{ label: 'Photos', slug: 'strategies/photos' },
						{ label: 'Documents', slug: 'strategies/documents' },
					],
				},
				{
					label: 'Watch',
					items: [
						{ label: 'Overview', slug: 'watch/overview' },
						{ label: 'Register and Start', slug: 'watch/register-and-start' },
						{ label: 'Pause, Resume and Stop', slug: 'watch/pause-resume-stop' },
						{ label: 'Stability Window', slug: 'watch/stability-window' },
						{ label: 'Recursive Watch', slug: 'watch/recursive-watch' },
						{ label: 'Lifecycle Guarantees', slug: 'watch/lifecycle-guarantees' },
					],
				},
				{
					label: 'Tray App',
					items: [
						{ label: 'Overview', slug: 'tray-app/overview' },
						{ label: 'Installation', slug: 'tray-app/installation' },
						{ label: 'Managing Watches', slug: 'tray-app/managing-watches' },
						{ label: 'Reapply Now', slug: 'tray-app/reapply-now' },
						{ label: 'Recursive Toggle', slug: 'tray-app/recursive-toggle' },
						{ label: 'Auto-launch Behavior', slug: 'tray-app/auto-launch-behavior' },
					],
				},
				{
					label: 'Reference',
					items: [
						{ label: 'CLI Commands', slug: 'reference/cli-commands' },
						{ label: 'File Classification', slug: 'reference/file-classification' },
						{ label: 'Configuration Reference', slug: 'reference/configuration-reference' },
						{ label: 'Platform Support', slug: 'reference/platform-support' },
					],
				},
				{
					label: 'Project',
					items: [
						{ label: 'Architecture', slug: 'project/architecture' },
						{ label: 'Contributing', slug: 'project/contributing' },
						{ label: 'Security', slug: 'project/security' },
						{ label: 'About', slug: 'project/about' },
					],
				},
			],
		}),
	],
});
