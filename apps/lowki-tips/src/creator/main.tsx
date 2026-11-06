import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import '../styles.css';
import { configError } from '../config';
import { ConfigError } from '../ui';
import Inbox from './Inbox';

const root = createRoot(document.getElementById('root')!);
root.render(configError ? <ConfigError message={configError} /> : <StrictMode><Inbox /></StrictMode>);
