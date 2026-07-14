import {
	createAliasUi,
	createAssignedUi,
	createFieldUi,
	createGetterUi,
	createNestedUi,
	createNewUi,
} from "./factory";

export function run(): void {
	const field = createFieldUi();
	field.orders.place();

	const getter = createGetterUi();
	getter.dashboard.open();

	const created = createNewUi();
	created.checkout.submit();

	const aliased = createAliasUi();
	aliased.profile.load();

	const nested = createNestedUi();
	nested.invoke.orders.placeNested();

	const assigned = createAssignedUi();
	assigned.settings.save();
}
