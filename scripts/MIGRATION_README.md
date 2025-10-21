Migration checklist: Move chats to separate repo and clean history

This folder contains helper scripts to export the `chats/` directory into a new repository
and to clean the main repository history by removing `chats/` from all commits.

Files:
- `migrate_chats_to_repo.py`: Python script that copies `chats/*.md` and compresses `chats/*.json` into an output folder. It can optionally `git init` and commit the results.
- `export_chats_repo.sh`: Wrapper to run the Python exporter and optionally create/push a new git repo.
- `clean_main_repo_history.sh`: Mirror-clone + git-filter-repo script to remove `chats/` from history and optionally force-push the cleaned repository (destructive).

Recommended safe flow:
1. Run a dry-run of the exporter to verify what will be created:
   python3 scripts/migrate_chats_to_repo.py --output-dir ../ccos-chats --dry-run

2. Export for real and init a local git repo in the output directory:
   ./scripts/export_chats_repo.sh --output-dir ../ccos-chats --fetch-lfs --init-git

3. Push `ccos-chats` to a new remote repository you control. Verify the archive is correct.

4. In the main repo, remove the original chat JSON files in a normal commit (non-destructive):
   git rm chats/*.json
   git commit -m "Move chats archive to ccos-chats"
   git push

5. If you want to reclaim server-side storage and LFS quota, run the history-clean script
   (this rewrites history and requires force pushing). Use the mirror mode first and inspect it:
   ./scripts/clean_main_repo_history.sh --mirror-dir /tmp/ccos-mirror --dry-run
   # when satisfied
   ./scripts/clean_main_repo_history.sh --mirror-dir /tmp/ccos-mirror --force-push

Notes:
- `clean_main_repo_history.sh` uses `git-filter-repo` and will overwrite history if you use `--force-push`.
- For LFS server-side garbage collection you may need to contact GitHub support after removing LFS pointers from history.
- Keep backups of your repository before running any destructive operations.

If you want, I can also prepare a sample `.gitattributes` and README for the new `ccos-chats` repo. 
