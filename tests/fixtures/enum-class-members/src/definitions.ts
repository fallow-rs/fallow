export enum Status {
    Active = 'active',
    Inactive = 'inactive',
    Pending = 'pending',
}

export class MyService {
    name: string = '';

    greet() { return 'hello'; }

    unusedMethod() { return 'unused'; }
}

export class ObjectPropertyService {
    usedThroughObject = 'used';

    usedThroughPropertyAlias = 'used';

    usedThroughObjectAlias = 'used';

    usedThroughDestructure = 'used';

    usedThroughComputed = 'used';

    usedThroughDynamicKey = 'used';

    usedThroughQualifiedConstructor = 'used';

    usedThroughCrossModuleContainer = 'used';

    unusedThroughObject = 'unused';
}
