import { getMe, getVersion } from '#lib/api';
import type { LayoutLoad } from './$types';

export const ssr = false;
export const prerender = false;

export const load: LayoutLoad = async () => {
  const [user, version] = await Promise.all([
    getMe().catch(() => null),
    getVersion().catch(() => null)
  ]);
  return { user, version };
};
