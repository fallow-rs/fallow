import { Component, inject as ngInject } from '@angular/core';
import { DataService } from './data.service';

@Component({
  selector: 'app-data-view',
  templateUrl: './data-view.component.html',
})
export class DataViewComponent {
  readonly injectedDataService = ngInject(DataService);

  constructor(public dataService: DataService) {}
}
