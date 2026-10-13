// Logos des services utilisés par les identifiants et les nœuds d'intégration.
// Ils sont définis ici, dans le code du site : ils ne se configurent pas.
//
// Chaque marque est un dessin SVG (grille 24 × 24) sur une tuile de couleur.
// Les tracés sont des silhouettes simplifiées, pas les fichiers officiels.

export interface Brand {
	label: string;
	/** Couleur de la tuile. */
	bg: string;
	/** Couleur du dessin. */
	fg: string;
	/** Contenu SVG (grille 24 × 24), couleurs via `currentColor` = `fg`. */
	svg: string;
}

export const brands = {
	postgres: {
		label: 'PostgreSQL',
		bg: '#336791',
		fg: '#ffffff',
		// Tête d'éléphant stylisée : crâne, oreille, trompe, œil.
		svg: `<path fill="currentColor" d="M12.3 3.2c-3.9 0-6.6 2.6-6.6 6.4 0 2 .7 3.5 1.5 4.7.5.8.9 1.6.9 2.4v2.6c0 .8.6 1.4 1.4 1.4h.9c.5 0 .9-.4.9-.9v-2.3c.4.1.9.1 1.3.1 3.8 0 6.7-2.5 6.7-6.1 0-.9-.2-1.7-.5-2.4.9-.3 1.6-.9 1.9-1.8-.8.3-1.6.3-2.4.1-1.2-2.1-3.3-3.2-6-3.2Z"/><circle cx="14.4" cy="9" r="1.1" fill="#336791"/><path fill="none" stroke="#336791" stroke-width="1.3" stroke-linecap="round" d="M9.6 12.6c.6 1.2 1.8 2.1 3.4 2.3"/>`,
	},
	mysql: {
		label: 'MySQL / MariaDB',
		bg: '#00758F',
		fg: '#F29111',
		// Le dauphin : une nageoire arquée au-dessus d'une vague.
		svg: `<path fill="currentColor" d="M3.5 15.5c2-5.3 5.6-8.3 10.3-8.6 2.1-.1 3.9.4 5.4 1.5-1.4-.1-2.6.2-3.6.9 1.6.6 2.9 1.7 3.9 3.3-1.8-.9-3.6-1.1-5.4-.6 1.1.9 1.8 2 2.1 3.4-1.9-1.5-4-2.1-6.3-1.8-2.5.3-4.6 1-6.4 1.9Z"/><path fill="#ffffff" d="M4 18.2c3.5-1.4 7-1.7 10.7-.9 1.9.4 3.7.3 5.3-.3-1.6 1.7-3.6 2.4-6.1 2.2-3.4-.3-6.7-.6-9.9-1Z"/>`,
	},
	oracle: {
		label: 'Oracle Database',
		bg: '#ffffff',
		fg: '#C74634',
		// Le « stade » rouge.
		svg: `<rect x="2.5" y="7.5" width="19" height="9" rx="4.5" fill="none" stroke="currentColor" stroke-width="2.6"/>`,
	},
	smtp: {
		label: 'Serveur SMTP',
		bg: '#475569',
		fg: '#ffffff',
		// Enveloppe.
		svg: `<rect x="3" y="5.5" width="18" height="13" rx="2.5" fill="none" stroke="currentColor" stroke-width="1.8"/><path fill="none" stroke="currentColor" stroke-width="1.8" stroke-linejoin="round" d="m3.8 7.5 8.2 6 8.2-6"/>`,
	},
	chat_webhook: {
		label: 'Webhook de messagerie',
		bg: '#0F766E',
		fg: '#ffffff',
		// Bulle de discussion.
		svg: `<path fill="currentColor" d="M12 3.5c-5 0-9 3.3-9 7.4 0 1.9.9 3.7 2.4 5l-1.2 4.1 4.4-2.1c1.1.3 2.2.5 3.4.5 5 0 9-3.3 9-7.5s-4-7.4-9-7.4Z"/>`,
	},
	Slack: {
		label: 'Slack',
		bg: '#ffffff',
		fg: '#4A154B',
		// Quatre « sucettes » colorées.
		svg: `<g><rect x="8.75" y="2" width="3.5" height="9.5" rx="1.75" fill="#36C5F0"/><circle cx="4.75" cy="9.75" r="1.75" fill="#36C5F0"/><rect x="12.5" y="8.75" width="9.5" height="3.5" rx="1.75" fill="#2EB67D"/><circle cx="14.25" cy="4.75" r="1.75" fill="#2EB67D"/><rect x="11.75" y="12.5" width="3.5" height="9.5" rx="1.75" fill="#E01E5A"/><circle cx="19.25" cy="14.25" r="1.75" fill="#E01E5A"/><rect x="2" y="11.75" width="9.5" height="3.5" rx="1.75" fill="#ECB22E"/><circle cx="9.75" cy="19.25" r="1.75" fill="#ECB22E"/></g>`,
	},
	Discord: {
		label: 'Discord',
		bg: '#5865F2',
		fg: '#ffffff',
		// Le visage « Clyde ».
		svg: `<path fill="currentColor" d="M18.6 6.3A15 15 0 0 0 15 5.2l-.5.9a14 14 0 0 0-5 0L9 5.2a15 15 0 0 0-3.6 1.1C3.1 9.8 2.5 13.2 2.8 16.6a15 15 0 0 0 4.5 2.3l.9-1.5c-.5-.2-1-.4-1.4-.7l.3-.3a10.7 10.7 0 0 0 9.8 0l.3.3c-.5.3-.9.5-1.4.7l.9 1.5a15 15 0 0 0 4.5-2.3c.4-3.9-.7-7.3-2.6-10.3ZM9.1 14.6c-.9 0-1.6-.8-1.6-1.8s.7-1.8 1.6-1.8c.9 0 1.6.8 1.6 1.8s-.7 1.8-1.6 1.8Zm5.8 0c-.9 0-1.6-.8-1.6-1.8s.7-1.8 1.6-1.8c.9 0 1.6.8 1.6 1.8s-.7 1.8-1.6 1.8Z"/>`,
	},
	'Microsoft Teams': {
		label: 'Microsoft Teams',
		bg: '#4B53BC',
		fg: '#ffffff',
		// Le « T » et la personne.
		svg: `<rect x="4" y="7" width="9.5" height="2.6" rx=".8" fill="currentColor"/><rect x="7.45" y="7" width="2.6" height="10.5" rx=".8" fill="currentColor"/><circle cx="17.3" cy="7.8" r="2" fill="currentColor"/><path fill="currentColor" d="M14.3 11.3h6c.4 0 .7.3.7.7v3.3c0 1.9-1.4 3.4-3.2 3.4-1.9 0-3.4-1.5-3.4-3.4v-3.3c0-.4-.2-.7-.1-.7Z"/>`,
	},
	Mattermost: {
		label: 'Mattermost',
		bg: '#1E325C',
		fg: '#ffffff',
		// Anneau ouvert et goutte.
		svg: `<path fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" d="M15.6 4.7a8 8 0 1 1-7.3 0"/><path fill="currentColor" d="M12.9 6.5c1.9 1.4 3 3.7 2.4 6a3.6 3.6 0 0 1-4.4 2.6 3.5 3.5 0 0 1-2.4-4.3c.6-2.3 2.6-3.8 4.4-4.3Z"/>`,
	},
	'Google Chat': {
		label: 'Google Chat',
		bg: '#ffffff',
		fg: '#00AC47',
		// Bulle verte à coin replié.
		svg: `<path fill="currentColor" d="M5.5 3.5h11c1.4 0 2.5 1.1 2.5 2.5v10c0 1.4-1.1 2.5-2.5 2.5H9l-4 3.3V6c0-1.4 1.1-2.5 2.5-2.5Z"/><path fill="#1E8E3E" d="M5 18.5V6c0-1.4 1.1-2.5 2.5-2.5V18l-2.5 3.3v-2.8Z"/><rect x="8" y="8" width="8" height="1.8" rx=".9" fill="#ffffff"/><rect x="8" y="11.5" width="6" height="1.8" rx=".9" fill="#ffffff"/>`,
	},
} satisfies Record<string, Brand>;

export type BrandKey = keyof typeof brands;

export const isBrand = (key: string | null | undefined): key is BrandKey => !!key && key in brands;

/** Service de messagerie reconnu d'après l'hôte d'une URL de webhook (même règle que le serveur). */
export function chatService(url: string): BrandKey | null {
	const m = /^https:\/\/([^/?#]+)(?:\/([^?#]*))?/i.exec(url.trim());
	if (!m) return null;
	const host = m[1].split('@').pop()!.split(':')[0].toLowerCase();
	const path = m[2] ?? '';
	const ends = (suffix: string) => host === suffix || host.endsWith(`.${suffix}`);
	if (ends('slack.com')) return 'Slack';
	if (ends('discord.com') || ends('discordapp.com')) return 'Discord';
	if (ends('webhook.office.com') || ends('logic.azure.com') || ends('powerautomate.com') || ends('office365.com')) return 'Microsoft Teams';
	if (ends('chat.googleapis.com')) return 'Google Chat';
	if (path.startsWith('hooks/') || host.includes('mattermost')) return 'Mattermost';
	return null;
}

/** Marque d'un identifiant : le service reconnu pour un webhook, sinon son type. */
export function brandOfCredential(kind: string, summary: Record<string, unknown> = {}): BrandKey {
	if (kind === 'chat_webhook' && isBrand(summary.service as string)) return summary.service as BrandKey;
	return isBrand(kind) ? kind : 'chat_webhook';
}

/** Marque d'un nœud d'intégration (type de nœud, et service choisi pour les messages). */
export function brandOfNode(nodeKind: string, service?: unknown): BrandKey | null {
	if (nodeKind.startsWith('postgres_')) return 'postgres';
	if (nodeKind.startsWith('mysql_')) return 'mysql';
	if (nodeKind.startsWith('oracle_')) return 'oracle';
	if (nodeKind === 'email_send') return 'smtp';
	if (nodeKind === 'chat_message') return isBrand(service as string) ? (service as BrandKey) : 'chat_webhook';
	return null;
}
