import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import './styles.css';
import { configError } from './config';
import { ConfigError } from './ui';

const root = createRoot(document.getElementById('root')!);
if (configError) {
  root.render(<ConfigError message={configError} />);
} else {
  // Loaded after the config check: the provider reads the config at render.
  void Promise.all([import('./providers'), import('./App')]).then(([{ default: Providers }, { default: App }]) =>
    root.render(
      <StrictMode>
        <Providers>
          <App />
        </Providers>
      </StrictMode>,
    ),
  );
}
