// Évalue une expression (avec un léger délai) sur les données du nœud.

import { engine } from '$lib/engine/client';
import type { Trial } from '$lib/engine/types';
import type { ExprContext } from '$lib/expressions/context';

export class TrialRunner {
	result = $state.raw<Trial | null>(null);
	row = $state(0);
	private seq = 0;
	private timer: ReturnType<typeof setTimeout> | undefined;

	run(src: string, template: boolean, ctx: ExprContext, delay = 120) {
		clearTimeout(this.timer);
		const id = ++this.seq;
		if (!src.trim() || (template && !src.includes('{{'))) {
			this.result = null;
			return;
		}
		this.timer = setTimeout(async () => {
			const nodes: Record<string, Uint8Array> = {};
			for (const n of ctx.nodes) if (n.bytes) nodes[n.name] = n.bytes;
			try {
				const r = await engine.tryExpression(src, template, {
					input: ctx.input?.bytes ?? null,
					nodes,
					vars: ctx.vars,
					row: this.row,
				});
				if (id === this.seq) this.result = r;
			} catch (e) {
				if (id === this.seq) this.result = { ok: false, error: e instanceof Error ? e.message : String(e), rows: 0 };
			}
		}, delay);
	}

	dispose() {
		clearTimeout(this.timer);
		this.seq++;
	}
}

/** Erreur due à l'absence de données en entrée (nœuds précédents pas encore exécutés). */
export function waitingForInput(r: Trial | null, ctx: ExprContext): boolean {
	return !!r && !r.ok && ctx.pendingInput && /Pas de données en entrée/.test(r.error ?? '');
}
