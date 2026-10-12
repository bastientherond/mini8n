<script lang="ts">
	// Mise en page des écrans d'accueil (connexion, inscription, premier workspace) :
	// panneau de présentation à gauche sur grand écran, formulaire à droite.
	import Braces from '@lucide/svelte/icons/braces';
	import Database from '@lucide/svelte/icons/database';
	import ShieldCheck from '@lucide/svelte/icons/shield-check';
	import Zap from '@lucide/svelte/icons/zap';
	import type { Snippet } from 'svelte';
	import Logo from './Logo.svelte';

	let { children }: { children: Snippet } = $props();

	const features = [
		{ icon: Zap, title: 'Rapide, partout', text: 'Moteur Rust compilé en WebAssembly : vos workflows tournent dans le navigateur comme sur le serveur.' },
		{ icon: Braces, title: 'Expressions JavaScript', text: '{{ $json.prix * 1.2 }}, autocomplétion et résultat en direct.' },
		{ icon: Database, title: 'Vos données, en Parquet', text: 'PostgreSQL, MySQL, Oracle, fichiers, e-mail, messageries…' },
		{ icon: ShieldCheck, title: 'Sécurisé par défaut', text: 'Secrets chiffrés, sessions durcies, accès réseau contrôlé.' },
	];
</script>

<div class="bg-background grid min-h-dvh lg:grid-cols-[minmax(0,1fr)_minmax(0,1.1fr)]">
	<!-- Présentation -->
	<aside class="bg-brand relative hidden overflow-hidden p-10 text-white lg:flex lg:flex-col">
		<!-- Motif : grille de points et halo -->
		<div
			class="pointer-events-none absolute inset-0 opacity-25"
			style="background-image: radial-gradient(rgb(255 255 255 / 0.5) 1px, transparent 1px); background-size: 22px 22px; mask-image: linear-gradient(to bottom, black, transparent 85%);"
		></div>
		<div class="pointer-events-none absolute -right-32 -bottom-32 size-[28rem] rounded-full bg-white/10 blur-3xl"></div>

		<div class="relative flex items-center gap-2.5 text-lg font-semibold">
			<Logo class="size-9 bg-white/15 ring-1 ring-white/25" />
			mini8n
		</div>

		<div class="relative my-auto max-w-md py-12">
			<h1 class="text-4xl leading-tight font-semibold tracking-tight">Automatisez vos flux de données, simplement.</h1>
			<p class="mt-4 text-base text-white/75">Connectez vos sources, transformez, envoyez. Visuellement, sans compromis sur la sécurité.</p>

			<!-- Mini workflow décoratif -->
			<div class="mt-10 flex items-center gap-0" aria-hidden="true">
				{#each ['Déclencheur', 'Filtrer', 'PostgreSQL'] as step, i (step)}
					{#if i > 0}<span class="h-px w-8 bg-white/40"></span>{/if}
					<span class="rounded-lg border border-white/25 bg-white/10 px-3 py-1.5 text-xs font-medium backdrop-blur-sm">{step}</span>
				{/each}
			</div>

			<ul class="mt-10 space-y-5">
				{#each features as f (f.title)}
					{@const Icon = f.icon}
					<li class="flex gap-3">
						<span class="flex size-8 shrink-0 items-center justify-center rounded-lg bg-white/15"><Icon class="size-4" /></span>
						<div>
							<p class="text-sm font-medium">{f.title}</p>
							<p class="text-sm text-white/70">{f.text}</p>
						</div>
					</li>
				{/each}
			</ul>
		</div>

		<p class="relative text-xs text-white/60">Open source · auto-hébergé · vos données restent chez vous.</p>
	</aside>

	<!-- Formulaire -->
	<main class="relative flex items-center justify-center p-6 sm:p-10">
		<div
			class="pointer-events-none absolute inset-x-0 top-0 h-64 opacity-60 lg:hidden"
			style="background: radial-gradient(60% 100% at 50% 0%, color-mix(in oklch, var(--primary) 18%, transparent), transparent);"
		></div>
		<div class="relative w-full max-w-sm">
			<div class="mb-8 flex items-center justify-center gap-2.5 text-lg font-semibold lg:hidden">
				<Logo class="size-9" />
				mini8n
			</div>
			{@render children()}
		</div>
	</main>
</div>
