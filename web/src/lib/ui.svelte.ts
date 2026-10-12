// État d'interface partagé (dialogues globaux) et actions réutilisées par la barre du haut,
// le menu de l'avatar et la palette de commandes.

import { toast } from 'svelte-sonner';
import { errorMessage } from './api';
import { router } from './router.svelte';
import { redactSecrets } from './sanitize';
import { session } from './session.svelte';
import { wf } from './workflow.svelte';

class Ui {
	palette = $state(false);
	newWorkspace = $state(false);
	newWorkflow = $state(false);
	shortcuts = $state(false);
	/** Workflow dont la suppression est en attente de confirmation. */
	confirmDelete = $state<{ id: string; name: string } | null>(null);
}

export const ui = new Ui();

/** Exécute une action en affichant l'erreur éventuelle dans un toast. */
export async function attempt<T>(action: () => Promise<T>, success?: string): Promise<T | undefined> {
	try {
		const result = await action();
		if (success) toast.success(success);
		return result;
	} catch (e) {
		toast.error(errorMessage(e));
		return undefined;
	}
}

export async function createAndOpenWorkflow(name: string, fromExample = false) {
	const ws = session.currentId;
	if (!ws) return;
	const id = await attempt(() => wf.createWorkflow(name, fromExample ? wf.example() : undefined));
	if (id) router.go({ name: 'editor', ws, workflow: id });
}

export async function importWorkflowFile(file: File) {
	const ws = session.currentId;
	if (!ws) return;
	if (file.size > 5 * 1024 * 1024) {
		toast.error('Fichier trop volumineux (5 Mo max.)');
		return;
	}
	const id = await attempt(async () => wf.importWorkflow(JSON.parse(await file.text())), 'Workflow importé');
	if (id) router.go({ name: 'editor', ws, workflow: id });
}

/** Ouvre le sélecteur de fichier puis importe le workflow choisi. */
export function pickWorkflowFile() {
	const input = Object.assign(document.createElement('input'), { type: 'file', accept: '.json,application/json' });
	input.onchange = () => {
		const file = input.files?.[0];
		if (file) void importWorkflowFile(file);
	};
	input.click();
}

export async function exportWorkflow(id: string) {
	const saved = await attempt(() => wf.exportable(id));
	if (!saved) return;
	const { workflow, redacted } = redactSecrets(saved);
	const blob = new Blob([JSON.stringify(workflow, null, 2)], { type: 'application/json' });
	const url = URL.createObjectURL(blob);
	const name = workflow.name.replace(/[^\p{L}\p{N}_-]+/gu, '_') || 'workflow';
	Object.assign(document.createElement('a'), { href: url, download: `${name}.mini8n.json` }).click();
	setTimeout(() => URL.revokeObjectURL(url), 1000);
	if (redacted > 0) toast.info(`${redacted} en-tête(s) sensible(s) masqué(s) dans l'export`);
}

export async function duplicateWorkflow(id: string) {
	const ws = session.currentId;
	const copy = await attempt(() => wf.duplicateWorkflow(id), 'Workflow dupliqué');
	if (copy && ws) router.go({ name: 'editor', ws, workflow: copy });
}

export async function deleteWorkflow(id: string) {
	const ws = session.currentId;
	const wasOpen = wf.workflowId === id;
	await attempt(() => wf.deleteWorkflow(id), 'Workflow supprimé');
	if (wasOpen && ws) router.go({ name: 'workflows', ws });
}

export async function logout(everywhere = false) {
	await wf.flush();
	await attempt(() => session.logout(everywhere));
	router.replace({ name: 'root' });
}
