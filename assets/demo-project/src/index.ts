import { Button } from './components/Button';
import { formatDate } from './utils/format';
import { ApiResponse } from './types/api';
import { Status } from './constants/status';
import { authenticate } from './services/auth';

export const app = {
  button: Button,
  date: formatDate(new Date()),
  status: [Status.Active, Status.Inactive, Status.Pending],
  login: authenticate,
};

export type Response = ApiResponse;
