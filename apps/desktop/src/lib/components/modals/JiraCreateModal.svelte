<script lang="ts">
  import type { JiraStatus } from '$lib/data';
  import { connectionsState } from '$lib/state/connections.svelte';
  import { modalsState, closeModal, patchModal } from '$lib/state/modals.svelte';
  import Dropdown, { type DropdownOption } from '$lib/components/ui/Dropdown.svelte';

  interface Props {
    onProjectChange: (key: string) => Promise<void> | void;
    onSubmit: () => Promise<void> | void;
  }
  let { onProjectChange, onSubmit }: Props = $props();

  const m = $derived(modalsState.jiraCreate);
  const jiraStatus = $derived<JiraStatus>(connectionsState.jira);

  const projectOpts = $derived<DropdownOption<string>[]>(
    m
      ? [
          { value: '', label: 'Select project…' },
          ...m.projects.map((p) => ({ value: p.key, label: `${p.key} · ${p.name}` }))
        ]
      : []
  );
  const issueTypeOpts = $derived<DropdownOption<string>[]>(
    m && m.issueTypes.length === 0
      ? [
          { value: 'Task', label: 'Task' },
          { value: 'Bug', label: 'Bug' },
          { value: 'Story', label: 'Story' }
        ]
      : (m?.issueTypes ?? []).map((t) => ({ value: t.name, label: t.name }))
  );
  const sprintOpts = $derived<DropdownOption<string>[]>(
    m
      ? [
          { value: '', label: 'No sprint' },
          ...m.sprints.map((s) => ({ value: String(s.id), label: s.name, hint: s.state }))
        ]
      : []
  );
  // "Unassigned" + the project-scoped assignable list. Email goes into the
  // dropdown's `hint` slot (faded right-aligned) so the displayName stays
  // the primary identifier.
  const assigneeOpts = $derived<DropdownOption<string>[]>(
    m
      ? [
          { value: '', label: 'Unassigned' },
          ...m.assignees.map((u) => ({
            value: u.account_id,
            label: u.display_name,
            hint: u.email_address ?? undefined
          }))
        ]
      : []
  );

  const canSubmit = $derived(
    !!m && !m.busy && m.projectKey.trim().length > 0 && m.issueTypeName.trim().length > 0 && m.summary.trim().length > 0
  );
</script>

{#if m}
  <div
    class="modal-backdrop"
    onclick={(e) => { if (e.target === e.currentTarget && !m.busy) closeModal('jiraCreate'); }}
    onkeydown={(e) => { if (e.key === 'Escape' && !m.busy) closeModal('jiraCreate'); }}
    role="dialog"
    aria-modal="true"
    tabindex="-1"
  >
    <div class="modal modal-wide">
      <header class="modal-head">
        <span class="conn-icon conn-icon--jira">J</span>
        <div>
          <div class="modal-title">New Jira issue</div>
          <div class="modal-sub">{jiraStatus.kind === 'connected' ? jiraStatus.user.workspace : ''}</div>
        </div>
        <button class="modal-close" onclick={() => closeModal('jiraCreate')} disabled={m.busy} aria-label="Close">
          <svg class="i" viewBox="0 0 24 24"><path d="M18 6 6 18M6 6l12 12" /></svg>
        </button>
      </header>
      <div class="modal-body">
        <div class="grid-2">
          <label class="field">
            <span class="field-label">Project</span>
            <Dropdown
              value={m.projectKey}
              options={projectOpts}
              onChange={(v) => onProjectChange(v)}
              disabled={m.busy}
              ariaLabel="Project"
              placeholder={m.projectsLoading ? 'Loading…' : 'Select project…'}
              width="100%"
            />
          </label>
          <label class="field">
            <span class="field-label">Issue type</span>
            <Dropdown
              value={m.issueTypeName}
              options={issueTypeOpts}
              onChange={(v) => patchModal('jiraCreate', { issueTypeName: v })}
              disabled={m.busy}
              ariaLabel="Issue type"
              width="100%"
            />
          </label>
        </div>
        <label class="field">
          <span class="field-label">Summary</span>
          <!-- svelte-ignore a11y_autofocus -->
          <input
            class="field-input"
            type="text"
            value={m.summary}
            oninput={(e) => patchModal('jiraCreate', { summary: (e.currentTarget as HTMLInputElement).value })}
            placeholder="Short, one-line title"
            disabled={m.busy}
            autofocus
          />
        </label>
        <label class="field">
          <span class="field-label">Description (markdown)</span>
          <textarea
            class="field-textarea"
            value={m.description}
            oninput={(e) => patchModal('jiraCreate', { description: (e.currentTarget as HTMLTextAreaElement).value })}
            placeholder="Optional — supports markdown paragraphs"
            disabled={m.busy}
          ></textarea>
        </label>
        <div class="grid-2">
          <label class="field">
            <span class="field-label">Assignee</span>
            <Dropdown
              value={m.assigneeAccountId}
              options={assigneeOpts}
              onChange={(v) => patchModal('jiraCreate', { assigneeAccountId: v })}
              disabled={m.busy || !m.projectKey}
              ariaLabel="Assignee"
              placeholder={m.assigneesLoading
                ? 'Loading users…'
                : m.projectKey
                  ? 'Unassigned'
                  : 'Select project first'}
              width="100%"
            />
          </label>
          <label class="field">
            <span class="field-label">Sprint</span>
            <Dropdown
              value={m.sprintId == null ? '' : String(m.sprintId)}
              options={sprintOpts}
              onChange={(v) => patchModal('jiraCreate', { sprintId: v ? Number(v) : null })}
              disabled={m.busy || m.sprints.length === 0}
              ariaLabel="Sprint"
              width="100%"
            />
          </label>
        </div>
        {#if m.error}<div class="modal-error">{m.error}</div>{/if}
      </div>
      <footer class="modal-foot">
        <button class="btn btn--ghost" onclick={() => closeModal('jiraCreate')} disabled={m.busy}>Cancel</button>
        <button class="btn btn--primary" onclick={() => void onSubmit()} disabled={!canSubmit}>
          {m.busy ? 'Creating…' : 'Create issue'}
        </button>
      </footer>
    </div>
  </div>
{/if}
