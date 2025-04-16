import streamlit as st
from streamlit_autorefresh import st_autorefresh
import asyncio
import websockets
import threading
import queue
import pandas as pd
import os
from dotenv import load_dotenv

import json

load_dotenv()


def is_json(myjson):
    try:
        json.loads(myjson)
    except ValueError as e:
        return False
    return True


# Refresh every second
st_autorefresh(interval=50, key="ws_refresher")

# Global queue to communicate between threads
if (
    "table_data_eth" not in st.session_state
    or "table_data_other" not in st.session_state
    or "table_data_btc" not in st.session_state
):
    st.session_state.table_data_eth = pd.DataFrame()
    st.session_state.table_data_btc = pd.DataFrame()
    st.session_state.table_data_other = pd.DataFrame()

if (
    "table_data_asks" not in st.session_state
    or "table_data_bids" not in st.session_state
):
    st.session_state.table_data_asks = pd.DataFrame()
    st.session_state.table_data_bids = pd.DataFrame()
    st.session_state.binance_u = 0


if "message_queue" not in st.session_state:
    st.session_state.message_queue = queue.Queue()


def start_ws_client(q):
    ws_server_host = os.getenv("WS_SERVER_HOST")
    ws_server_port = os.getenv("WS_SERVER_PORT")
    assert ws_server_host and ws_server_port

    async def ws_loop():
        print(f"ws server port: {ws_server_host}")
        print(f"ws server host: {ws_server_port}")
        uri = f"ws://{ws_server_host}:{ws_server_port}"
        try:
            async with websockets.connect(uri) as websocket:
                while True:
                    message = await websocket.recv()
                    if is_json(str(message)):
                        q.put(str(message))
                    else:
                        print(f"discarding -->{message}")
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
    try:
        json_loaded = json.loads(st.session_state.ws_data)
    except Exception as e:
        print("it is not parsing json")
        st.warning(st.session_state.ws_data)
    else:
        # binance data
        if "BinanceDepthUpdate" in json_loaded:
            # Extract bids and asks
            json_list = json_loaded["BinanceDepthUpdate"]
            bids = json_list["b"]
            asks = json_list["a"]
            st.session_state.binance_u = json_list["u"]

            # Convert to pandas DataFrames
            st.session_state.table_data_bids = pd.DataFrame(
                bids, columns=["Price", "Quantity"]
            ).astype(float)
            st.session_state.table_data_asks = pd.DataFrame(
                asks, columns=["Price", "Quantity"]
            ).astype(float)

            st.session_state.table_data_bids = (
                st.session_state.table_data_bids.sort_values(
                    by="Price", ascending=False
                )
            )
            st.session_state.table_data_asks = (
                st.session_state.table_data_asks.sort_values(by="Price", ascending=True)
            )

            # Display update identifier
            # Display tables

        # coinbase check
        if "Snapshot" in json_loaded:
            snapshot = json_loaded["Snapshot"]
            if len(snapshot) > 0:
                first_elem = snapshot[0]
                if "product_id" not in first_elem:
                    st.session_state.table_data_other = pd.json_normalize(snapshot)
                else:
                    ident = first_elem["product_id"]
                    if ident == "BTC-USD":
                        st.session_state.table_data_btc = pd.json_normalize(snapshot)
                    elif ident == "ETH-USD":
                        st.session_state.table_data_eth = pd.json_normalize(snapshot)
            else:
                pass
                # st.session_state.table_data_other = pd.json_normalize(json_loaded)
        else:
            pass
            # st.session_state.table_data_other = pd.json_normalize(json_loaded)

# Show the data
st.title("📡 Real-Time WebSocket Stream")
st.table(st.session_state.table_data_eth)
st.table(st.session_state.table_data_btc)
st.table(st.session_state.table_data_other)
st.subheader("Bids (Buy Orders)")
st.dataframe(st.session_state.table_data_bids)
st.subheader("Asks (Sell Orders)")
st.dataframe(st.session_state.table_data_asks)
