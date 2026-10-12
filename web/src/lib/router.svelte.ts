// Routage minimal par hash (#/w/<workspace>/workflows/<id>…) : liens partageables,
// boutons précédent/suivant du navigateur, aucune configuration serveur.

export type Route =
	| { name: 'root' }
	| { name: 'account' }
	| { name: 'home'; ws: string }
	| { name: 'workflows'; ws: string }
	| { name: 'editor'; ws: string; workflow: string }
	| { name: 'members'; ws: string }
	| { name: 'settings'; ws: string }
	| { name: 'executions'; ws: string }
	| { name: 'credentials'; ws: string }
	| { name: 'files'; ws: string }
	| { name: 'form'; token: string };

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const TOKEN = /^[A-Za-z0-9_-]{32,128}$/;

export function parse(hash: string): Route {
	const parts = hash.replace(/^#\/?/, '').split('/').filter(Boolean);
	if (parts[0] === 'account' && parts.length === 1) return { name: 'account' };
	if (parts[0] === 'f' && TOKEN.test(parts[1] ?? '')) return { name: 'form', token: parts[1] };
	if (parts[0] !== 'w' || !UUID.test(parts[1] ?? '')) return { name: 'root' };
	const ws = parts[1].toLowerCase();
	switch (parts[2]) {
		case undefined:
			return { name: 'home', ws };
		case 'workflows':
			if (parts[3] === undefined) return { name: 'workflows', ws };
			return UUID.test(parts[3]) ? { name: 'editor', ws, workflow: parts[3].toLowerCase() } : { name: 'workflows', ws };
		case 'members':
			return { name: 'members', ws };
		case 'settings':
			return { name: 'settings', ws };
		case 'executions':
			return { name: 'executions', ws };
		case 'credentials':
			return { name: 'credentials', ws };
		case 'files':
			return { name: 'files', ws };
		default:
			return { name: 'home', ws };
	}
}

export function format(route: Route): string {
	switch (route.name) {
		case 'root':
			return '/';
		case 'account':
			return '/account';
		case 'home':
			return `/w/${route.ws}`;
		case 'workflows':
			return `/w/${route.ws}/workflows`;
		case 'editor':
			return `/w/${route.ws}/workflows/${route.workflow}`;
		case 'members':
			return `/w/${route.ws}/members`;
		case 'settings':
			return `/w/${route.ws}/settings`;
		case 'executions':
			return `/w/${route.ws}/executions`;
		case 'credentials':
			return `/w/${route.ws}/credentials`;
		case 'files':
			return `/w/${route.ws}/files`;
		case 'form':
			return `/f/${route.token}`;
	}
}

export const href = (route: Route) => `#${format(route)}`;

class Router {
	route = $state<Route>(parse(location.hash));

	constructor() {
		window.addEventListener('hashchange', () => {
			this.route = parse(location.hash);
		});
	}

	go(route: Route) {
		const target = format(route);
		if (location.hash.slice(1) === target) this.route = parse(location.hash);
		else location.hash = target;
	}

	/** Remplace l'entrée d'historique courante (redirections). */
	replace(route: Route) {
		history.replaceState(history.state, '', `#${format(route)}`);
		this.route = route;
	}
}

export const router = new Router();
