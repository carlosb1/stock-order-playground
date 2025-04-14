import asyncio
import websockets
import datetime

connected_clients = set()


async def send_periodic_updates():
    while True:
        if connected_clients:
            message = f"Update: {datetime.datetime.now().isoformat()}"
            print(f"Sending to {len(connected_clients)} clients: {message}")
            await asyncio.gather(
                *[conn.send(message) for conn in connected_clients],
                return_exceptions=True,
            )
        await asyncio.sleep(5)  # <-- Change the interval here (5 seconds)


async def handle_connection(websocket):
    connected_clients.add(websocket)
    print(f"Client connected: {websocket.remote_address}")

    try:
        await websocket.wait_closed()
    finally:
        connected_clients.remove(websocket)
        print(f"Client disconnected: {websocket.remote_address}")


async def main():
    print("Starting WebSocket server at ws://localhost:9001")
    async with websockets.serve(handle_connection, "localhost", 9001):
        await send_periodic_updates()


if __name__ == "__main__":
    asyncio.run(main())
