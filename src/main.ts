import './styles.css';
import { renderTunnelList } from './views/TunnelList';
import { renderTunnelForm } from './views/TunnelForm';
import { renderTunnelStats } from './views/TunnelStats';
import { renderErrorConsole } from './views/ErrorConsole';

type Route =
  | { view: 'list' }
  | { view: 'add' }
  | { view: 'edit'; id: string }
  | { view: 'stats'; id: string }
  | { view: 'console' };

let currentRoute: Route = { view: 'list' };
let cleanupFn: (() => void) | null = null;

export function navigate(route: Route) {
  if (cleanupFn) {
    cleanupFn();
    cleanupFn = null;
  }
  currentRoute = route;
  render();
}

// Bridge for tray menu to navigate the frontend
(window as any).__navigateTo = (view: string) => {
  navigate({ view } as Route);
};

function render() {
  const app = document.getElementById('app')!;
  app.innerHTML = '';

  switch (currentRoute.view) {
    case 'list':
      renderTunnelList(app);
      break;
    case 'add':
      renderTunnelForm(app, null);
      break;
    case 'edit':
      renderTunnelForm(app, currentRoute.id);
      break;
    case 'stats':
      cleanupFn = renderTunnelStats(app, currentRoute.id);
      break;
    case 'console':
      cleanupFn = renderErrorConsole(app);
      break;
  }
}

render();
