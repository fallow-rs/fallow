import {
	CheckoutPage,
	DashboardPage,
	OrdersPage,
	ProfilePage,
	SettingsPage,
} from "./pages";

export class InvokerFactory {
	readonly orders: OrdersPage = new OrdersPage();
	private dash?: DashboardPage;
	get dashboard(): DashboardPage {
		return (this.dash ??= new DashboardPage());
	}
}

// Field member-read of a separately-constructed instance (issue #1858 headline).
export function createFieldUi() {
	const factory = new InvokerFactory();
	return { orders: factory.orders };
}

// Getter member-read.
export function createGetterUi() {
	const factory = new InvokerFactory();
	return { dashboard: factory.dashboard };
}

// Direct `new Class()` in the literal.
export function createNewUi() {
	return { checkout: new CheckoutPage() };
}

// Local `const` alias to `new Class()`.
export function createAliasUi() {
	const profile = new ProfilePage();
	return { profile };
}

// Nested object literal, consumed as `ui.invoke.orders.placeNested()`.
export function createNestedUi() {
	const factory = new InvokerFactory();
	return { invoke: { orders: factory.orders } };
}

// Assigned-then-returned (`const ui = {...}; return ui;`).
export function createAssignedUi() {
	const settings = new SettingsPage();
	const ui = { settings };
	return ui;
}
