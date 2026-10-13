// Vue du canevas : translation et zoom, conversions écran ↔ canevas.

import type { Point, Rect } from './router';

export const MIN_ZOOM = 0.2;
export const MAX_ZOOM = 2;

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

export class Viewport {
	x = $state(0);
	y = $state(0);
	zoom = $state(1);
	/** Taille de l'élément du canevas (pixels écran). */
	width = $state(0);
	height = $state(0);
	/** Coin supérieur gauche de l'élément à l'écran. */
	left = 0;
	top = 0;

	toCanvas(screen: Point): Point {
		return { x: (screen.x - this.left - this.x) / this.zoom, y: (screen.y - this.top - this.y) / this.zoom };
	}

	/** Zoom autour d'un point écran (molette, pincement). */
	zoomAt(screen: Point, zoom: number) {
		const z = clamp(zoom, MIN_ZOOM, MAX_ZOOM);
		const p = this.toCanvas(screen);
		this.zoom = z;
		this.x = screen.x - this.left - p.x * z;
		this.y = screen.y - this.top - p.y * z;
	}

	zoomBy(factor: number) {
		this.zoomAt({ x: this.left + this.width / 2, y: this.top + this.height / 2 }, this.zoom * factor);
	}

	panBy(dx: number, dy: number) {
		this.x += dx;
		this.y += dy;
	}

	/** Centre la vue sur un point du canevas. */
	centerOn(p: Point) {
		this.x = this.width / 2 - p.x * this.zoom;
		this.y = this.height / 2 - p.y * this.zoom;
	}

	/** Cadre un ensemble de nœuds. */
	fit(bounds: Rect | null, padding = 60, maxZoom = 1.1) {
		if (!bounds || this.width === 0) return;
		const z = clamp(
			Math.min((this.width - padding * 2) / Math.max(bounds.w, 1), (this.height - padding * 2) / Math.max(bounds.h, 1), maxZoom),
			MIN_ZOOM,
			MAX_ZOOM,
		);
		this.zoom = z;
		this.centerOn({ x: bounds.x + bounds.w / 2, y: bounds.y + bounds.h / 2 });
	}

	/** Partie visible du canevas. */
	visible(): Rect {
		return { x: -this.x / this.zoom, y: -this.y / this.zoom, w: this.width / this.zoom, h: this.height / this.zoom };
	}
}

export function boundsOf(rects: Rect[]): Rect | null {
	if (rects.length === 0) return null;
	const x = Math.min(...rects.map((r) => r.x));
	const y = Math.min(...rects.map((r) => r.y));
	const x2 = Math.max(...rects.map((r) => r.x + r.w));
	const y2 = Math.max(...rects.map((r) => r.y + r.h));
	return { x, y, w: x2 - x, h: y2 - y };
}
