# 👾 Processbot

### Available Commands (post as a comment in the relevant PR thread) 
- `bot merge` to automatically merge it once checks pass (if approvals have been
  given)
- `bot merge cancel` to cancel a pending `bot merge`
- `bot compare substrate` to see a diff between current branch's Substrate
  version and the latest Polkadot release substrate.

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
