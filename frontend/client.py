import streamlit as st
from streamlit_autorefresh import st_autorefresh
import asyncio
import websockets
import threading
import queue

# Refresh every second
st_autorefresh(interval=50, key="ws_refresher")

# Global queue to communicate between threads
if "message_queue" not in st.session_state:
    st.session_state.message_queue = queue.Queue()


def start_ws_client(q):
    async def ws_loop():
        uri = "ws://localhost:9001"
        try:
            async with websockets.connect(uri) as websocket:
                while True:
                    message = await websocket.recv()
                    print(str(message))
                    q.put(str(message))
        except Exception as e:
            q.put(f"Connection error: {e}")

    asyncio.new_event_loop().run_until_complete(ws_loop())


# Start the WebSocket client thread
if "ws_thread_started" not in st.session_state:
    threading.Thread(
        target=start_ws_client, args=(st.session_state.message_queue,), daemon=True
    ).start()
    st.session_state.ws_thread_started = True

# Process new messages from the queue
if not st.session_state.message_queue.empty():
    st.session_state.ws_data = st.session_state.message_queue.get()

# Show the data
st.title("📡 Real-Time WebSocket Stream")
if "ws_data" in st.session_state:
    st.write(f"📨 Latest message: `{st.session_state.ws_data}`")
