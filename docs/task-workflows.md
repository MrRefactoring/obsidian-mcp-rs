# Task workflows

Tasks live in the original Markdown notes. These requests work with Obsidian closed. The examples assume the agent can use `list_tasks`, `create_task`, `update_task`, `preview_task_batch`, and `apply_task_batch`.

## Morning review

> Review my tasks in this vault. Show incomplete tasks due before today, then tasks due or scheduled for today. Keep tasks from today's daily note in a separate section. Include the source note and line for each group.

Use the local reference date returned by `list_tasks`. Use the `today` scope for tasks in the daily note and the `today` date match for tasks due or scheduled today. These are different lists.

## Weekly plan

> Show incomplete tasks due through the end of this week. Put blocked work in its own section. Suggest which due dates to change. Show a preview and wait for my approval before you apply the changes.

Use the date range filter with explicit dates. Show each source, date change, and destination note. Use the task references returned by `list_tasks` and pass the same operations and plan ID to `apply_task_batch`.

## Inbox triage

> List open tasks in Inbox. Suggest a project note for each task. Preview a batch move for the ones with clear destinations. Ask me about the others.

Check the target note and heading before you make the plan. If the batch stops after some files change, use the per-file report and inspect affected notes before making another plan. A partial run can keep a task at both its source and destination.

## Meeting actions

> Read this meeting note. Extract only the agreed actions and include the owner or due date when the note gives one. Show the target project note and heading before you add the tasks.

Use `create_task` for each approved action. To update an existing task, first use `list_tasks` to get its path, line, and revision. If the note has changed, refresh the task list before you edit. Never choose a task by description alone when there are duplicates.

## Supported Tasks behavior

The tools read Obsidian Tasks emoji fields. They support daily, weekly, and monthly recurrence with optional positive intervals, with or without `when done`. Completing a task with a different recurrence expression or an on-completion action requires Obsidian Tasks. The tools preserve syntax they cannot interpret and do not execute Tasks code-block queries or plugins.

References: [Obsidian Tasks documentation](https://publish.obsidian.md/tasks/) and [custom statuses and recurrence](https://publish.obsidian.md/tasks/Getting%20Started/Statuses/Recurring%20Tasks%20and%20Custom%20Statuses).
