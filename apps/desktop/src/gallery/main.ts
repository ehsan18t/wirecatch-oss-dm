// The component gallery: a dev-only page (`/gallery/` on the dev server) showing every piece of @wirecatch/ui in both schemes and every accent. Vite builds only the HTML pages at the app's root, so it never reaches a production build; scripts/check-bundle.mjs checks that.
import './gallery.css';
import { mount } from 'svelte';
import Gallery from './Gallery.svelte';

mount(Gallery, { target: document.body });
