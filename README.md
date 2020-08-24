# 👾 Processbot

### Available Commands (post as a comment in the relevant PR thread) 
- `bot merge` to automatically merge it once checks pass (if approvals have been
  given)
- `bot merge force` to attempt merge without waiting for checks (if approvals
  have been given)
- `bot merge cancel` to cancel a pending `bot merge`
- `bot compare substrate` to see a diff between current branch's Substrate
  version and the latest Polkadot release substrate.

### FAQ
- Who are `core-devs`? 
	- https://github.com/orgs/paritytech/teams/core-devs/members

- Who are `substrateteamleads`?
	- https://github.com/orgs/paritytech/teams/substrateteamleads/members

- What is a project column and how do I attach one?
	- A project column is necessary for Processbot to identify a project owner.
	- Approval from a relevant project owner removes the need for further approvals.
	- A pull request can be attached to a project column using the Github web UI (similar to attaching a label):

		- No project *(cannot be recognised)*

		![](https://github.com/paritytech/parity-processbot/blob/master/no-project.png)
	
		- A project but no column *(cannot be recognised)*
	
		![](https://github.com/paritytech/parity-processbot/blob/master/no-column.png)
	
		- The project `parity-processbot` and column `general` *(will be recognised)*
	
		![](https://github.com/paritytech/parity-processbot/blob/master/proj-column.png)

## Repository Configuration 

### `Process.json` file
In the repository's root directory. Eg:

```
[{
	"project_name": "Networking",
	"owner": "tomaka",
	"matrix_room_id": "!vUADSGcyXmxhKLeDsW:matrix.parity.io"
},
{	"project_name": "Client",
	"owner": "gnunicorn",
	"matrix_room_id": "!aenJixaHcSKbJOWxYk:matrix.parity.io"
},
{
	"project_name": "Runtime",
	"owner": "gavofyork",
	"matrix_room_id": "!yBKstWVBkwzUkPslsp:matrix.parity.io"
},
{
	"project_name": "Consensus",
	"owner": "andresilva",
	"matrix_room_id": "!XdNWDTfVNFVixljKZU:matrix.parity.io"
},
{
	"project_name": "Smart Contracts",
	"owner": "pepyakin",
	"matrix_room_id": "!yBKstWVBkwzUkPslsp:matrix.parity.io"
}]
```
