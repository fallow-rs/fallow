import { addColumn } from '../../utils/add-column.util';

export class Init1700000000000 {
  name = 'Init1700000000000';

  public async up(): Promise<void> {
    await addColumn('user', 'email');
  }
}
