import { redirect } from '@sveltejs/kit';
import { listUsers } from '$lib/api';
import type { PageLoad } from './$types';

export const load: PageLoad = async ({ parent, depends }) => {
  depends('app:users');
  const { user } = await parent();
  if (!user) throw redirect(303, '/login');
  if (user.role !== 'admin') throw redirect(303, '/');
  const users = await listUsers();
  return { user, users };
};
