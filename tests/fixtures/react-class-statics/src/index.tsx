import { createRoot } from 'react-dom/client';
import { Badge, Greeting, Panel, UserModel } from './components';

const user = new UserModel();
user.save();

createRoot(document.body).render(
  <Greeting>
    <Panel />
    <Badge />
  </Greeting>,
);
