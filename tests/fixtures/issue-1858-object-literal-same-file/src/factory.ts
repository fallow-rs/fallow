import { SameFilePage } from "./pages";

function createUi() {
	return { page: new SameFilePage() };
}

export function run(): void {
	const ui = createUi();
	ui.page.go();
}
