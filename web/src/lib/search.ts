import { z } from 'zod';

export const SIDE_VIEWS = ['project', 'find', 'debug', 'peripherals', 'flash'] as const;
export const PANELS = ['build', 'command', 'find', 'mem1', 'mem2', 'mem3', 'mem4', 'uart', 'logic'] as const;

export type SideView = (typeof SIDE_VIEWS)[number];
export type PanelId = (typeof PANELS)[number];

export const projectSearchSchema = z.object({
  file: z.string().min(1).optional().catch(undefined),
  line: z.coerce.number().int().min(1).optional().catch(undefined),
  view: z.enum(SIDE_VIEWS).optional().catch(undefined),
  panel: z.enum(PANELS).optional().catch(undefined),
  mode: z.enum(['edit', 'debug']).optional().catch(undefined),
  mem: z
    .string()
    .regex(/^(0x)?[0-9a-fA-F]{1,8}$/)
    .optional()
    .catch(undefined),
});

export type ProjectSearch = z.infer<typeof projectSearchSchema>;
