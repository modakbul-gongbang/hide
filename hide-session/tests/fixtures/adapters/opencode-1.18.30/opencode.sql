-- OpenCode 1.18.30's own tables, as `sqlite3 opencode.db .schema` prints them
-- (`project` cut to the column a session's key needs).
CREATE TABLE `project` (`id` text PRIMARY KEY);
INSERT INTO project VALUES ('prj_1');
CREATE TABLE `session` (
          `id` text PRIMARY KEY,
          `project_id` text NOT NULL,
          `workspace_id` text,
          `parent_id` text,
          `slug` text NOT NULL,
          `directory` text NOT NULL,
          `path` text,
          `title` text NOT NULL,
          `version` text NOT NULL,
          `share_url` text,
          `summary_additions` integer,
          `summary_deletions` integer,
          `summary_files` integer,
          `summary_diffs` text,
          `metadata` text,
          `cost` real DEFAULT 0 NOT NULL,
          `tokens_input` integer DEFAULT 0 NOT NULL,
          `tokens_output` integer DEFAULT 0 NOT NULL,
          `tokens_reasoning` integer DEFAULT 0 NOT NULL,
          `tokens_cache_read` integer DEFAULT 0 NOT NULL,
          `tokens_cache_write` integer DEFAULT 0 NOT NULL,
          `revert` text,
          `permission` text,
          `agent` text,
          `model` text,
          `time_created` integer NOT NULL,
          `time_updated` integer NOT NULL,
          `time_compacting` integer,
          `time_archived` integer,
          CONSTRAINT `fk_session_project_id_project_id_fk` FOREIGN KEY (`project_id`) REFERENCES `project`(`id`) ON DELETE CASCADE
        );
CREATE INDEX `session_project_idx` ON `session` (`project_id`);
CREATE INDEX `session_parent_idx` ON `session` (`parent_id`);
CREATE TABLE `message` (
          `id` text PRIMARY KEY,
          `session_id` text NOT NULL,
          `time_created` integer NOT NULL,
          `time_updated` integer NOT NULL,
          `data` text NOT NULL,
          CONSTRAINT `fk_message_session_id_session_id_fk` FOREIGN KEY (`session_id`) REFERENCES `session`(`id`) ON DELETE CASCADE
        );
CREATE INDEX `message_session_time_created_id_idx` ON `message` (`session_id`,`time_created`,`id`);
CREATE TABLE `part` (
          `id` text PRIMARY KEY,
          `message_id` text NOT NULL,
          `session_id` text NOT NULL,
          `time_created` integer NOT NULL,
          `time_updated` integer NOT NULL,
          `data` text NOT NULL,
          CONSTRAINT `fk_part_message_id_message_id_fk` FOREIGN KEY (`message_id`) REFERENCES `message`(`id`) ON DELETE CASCADE
        );
CREATE INDEX `part_message_id_id_idx` ON `part` (`message_id`,`id`);
CREATE INDEX `part_session_idx` ON `part` (`session_id`);

INSERT INTO session (id, project_id, slug, directory, title, version, time_created, time_updated) VALUES ('ses_0a1b2c3d4e5f60718293a4b5c6', 'prj_1', 'brave-otter', '/work/app', '요청 보기 만들기', '1.18.30', 1790989200000, 1790989331000);
INSERT INTO message VALUES ('msg_01', 'ses_0a1b2c3d4e5f60718293a4b5c6', 1790989200000, 1790989200000, '{"role": "user", "time": {"created": 1790989200000}, "agent": "build", "model": {"providerID": "p", "modelID": "m"}}');
INSERT INTO part VALUES ('prt_01', 'msg_01', 'ses_0a1b2c3d4e5f60718293a4b5c6', 1790989200000, 1790989200000, '{"type": "text", "text": "요청 보기를 만들어줘\n긴 요청의 둘째 줄"}');
INSERT INTO message VALUES ('msg_02', 'ses_0a1b2c3d4e5f60718293a4b5c6', 1790989201000, 1790989201000, '{"parentID": "msg_x", "role": "assistant", "mode": "build", "agent": "build", "path": {"cwd": "/work/app", "root": "/work/app"}, "cost": 0, "tokens": {}, "modelID": "m", "providerID": "p", "time": {"created": 1790989201000, "completed": 1790989231000}, "finish": "tool-calls"}');
INSERT INTO part VALUES ('prt_02', 'msg_02', 'ses_0a1b2c3d4e5f60718293a4b5c6', 1790989201000, 1790989201000, '{"type": "step-start"}');
INSERT INTO part VALUES ('prt_03', 'msg_02', 'ses_0a1b2c3d4e5f60718293a4b5c6', 1790989220000, 1790989220000, '{"type": "tool", "tool": "bash", "callID": "c1", "state": {"status": "completed", "input": {"command": "bash scripts/ship.sh"}, "output": "https://github.com/acme/app/pull/12\n", "metadata": {}, "title": "ship", "time": {"start": 1790989220000, "end": 1790989230000}}}');
INSERT INTO message VALUES ('msg_03', 'ses_0a1b2c3d4e5f60718293a4b5c6', 1790989240000, 1790989240000, '{"role": "user", "time": {"created": 1790989240000}, "agent": "build", "model": {"providerID": "p", "modelID": "m"}}');
INSERT INTO part VALUES ('prt_04', 'msg_03', 'ses_0a1b2c3d4e5f60718293a4b5c6', 1790989240000, 1790989240000, '{"type": "text", "text": "Summarize the conversation so far", "synthetic": true}');
INSERT INTO message VALUES ('msg_04', 'ses_0a1b2c3d4e5f60718293a4b5c6', 1790989260000, 1790989260000, '{"role": "user", "time": {"created": 1790989260000}, "agent": "build", "model": {"providerID": "p", "modelID": "m"}}');
INSERT INTO part VALUES ('prt_05', 'msg_04', 'ses_0a1b2c3d4e5f60718293a4b5c6', 1790989260000, 1790989260000, '{"type": "text", "text": "HCOORD_REQUEST r_1 from ci-lead (p_7)\nreply: hcoord request reply r_1 --as p_2 --body <answer> | escalate: hcoord request escalate r_1 --actor p_2\nCI 다시 봐줘"}');
INSERT INTO message VALUES ('msg_05', 'ses_0a1b2c3d4e5f60718293a4b5c6', 1790989320000, 1790989320000, '{"role": "user", "time": {"created": 1790989320000}, "agent": "build", "model": {"providerID": "p", "modelID": "m"}}');
INSERT INTO part VALUES ('prt_06', 'msg_05', 'ses_0a1b2c3d4e5f60718293a4b5c6', 1790989320000, 1790989320000, '{"type": "file", "mime": "image/png", "filename": "clipboard.png", "url": "data:image/png;base64,iVBORw0KGgo=", "source": {}}');
INSERT INTO message VALUES ('msg_06', 'ses_0a1b2c3d4e5f60718293a4b5c6', 1790989330000, 1790989330000, '{"parentID": "msg_x", "role": "assistant", "mode": "build", "agent": "build", "path": {"cwd": "/work/app", "root": "/work/app"}, "cost": 0, "tokens": {}, "modelID": "m", "providerID": "p", "time": {"created": 1790989330000, "completed": 1790989331000}, "finish": "stop"}');
INSERT INTO part VALUES ('prt_07', 'msg_06', 'ses_0a1b2c3d4e5f60718293a4b5c6', 1790989330000, 1790989330000, '{"type": "text", "text": "PR을 열었습니다: https://github.com/acme/app/pull/12\n예전 것은 https://github.com/acme/app/pull/99 입니다", "time": {"start": 1790989330000, "end": 1790989331000}}');
INSERT INTO part VALUES ('prt_08', 'msg_06', 'ses_0a1b2c3d4e5f60718293a4b5c6', 1790989331000, 1790989331000, '{"type": "step-finish", "reason": "stop", "tokens": {}, "cost": 0}');
