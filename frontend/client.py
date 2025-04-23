from datetime import datetime, timedelta

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

    # Estilos condicionales


def style_row(row):
    styles = ["color: black; border: 1px solid #ccc"] * len(row)
    if row["Match"]:
        styles = [
            "background-color: #fff799; color: black; font-weight: bold; border: 1px solid #666"
        ] * len(row)
    else:
        if pd.notnull(row["Bid Quantity"]):
            i = row.index.get_loc("Bid Quantity")
            styles[i] = (
                "background-color: #d4f7d4; color: black; border: 1px solid #ccc"
            )
        if pd.notnull(row["Bid Price"]):
            i = row.index.get_loc("Bid Price")
            styles[i] = (
                "background-color: #d4f7d4; color: black; border: 1px solid #ccc"
            )
        if pd.notnull(row["Ask Quantity"]):
            i = row.index.get_loc("Ask Quantity")
            styles[i] = (
                "background-color: #f7d4d4; color: black; border: 1px solid #ccc"
            )
        if pd.notnull(row["Ask Price"]):
            i = row.index.get_loc("Ask Price")
            styles[i] = (
                "background-color: #f7d4d4; color: black; border: 1px solid #ccc"
            )
    return styles


# Refresh every second
st_autorefresh(interval=1000, key="ws_refresher")

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

if "styled_order_book" not in st.session_state:
    st.session_state.styled_order_book = pd.DataFrame()


def start_ws_client(q):
    ws_server_host = os.getenv("WS_SERVER_HOST")
    ws_server_port = os.getenv("WS_SERVER_PORT")
    assert ws_server_host and ws_server_port

    async def ws_loop():
        while True:
            print(f"ws server port: {ws_server_host}")
            print(f"ws server host: {ws_server_port}")
            uri = f"ws://{ws_server_host}:{ws_server_port}"
            try:
                async with websockets.connect(uri) as websocket:
                    # TODO  set up logic for choose type of book order!ccc
                    start = datetime.now()
                    while True:
                        stop = datetime.now()
                        if stop - start > timedelta(seconds=20):
                            print("Sending ping message")
                            await websocket.send("ping")
                            start = datetime.now()
                        message = await websocket.recv()
                        if is_json(str(message)):
                            q.put(str(message))
                        else:
                            print(f"discarding -->{message}")

            except Exception as e:
                print(f"Connection error: {e}")

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
        print(st.session_state.ws_data)
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

        elif "Snapshot" in json_loaded:
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
                st.session_state.table_data_other = pd.json_normalize(json_loaded)
        else:
            st.session_state.table_data_other = pd.json_normalize(json_loaded)

# Show the data
st.title("📡 Real-Time WebSocket Stream")

# Unificar order book (asks y bids) en una sola tabla
merged_order_book = pd.merge(
    st.session_state.table_data_bids.rename(
        columns={"Price": "Bid Price", "Quantity": "Bid Quantity"}
    ),
    st.session_state.table_data_asks.rename(
        columns={"Price": "Ask Price", "Quantity": "Ask Quantity"}
    ),
    how="outer",
    left_index=True,
    right_index=True,
)

if "Bid Price" in merged_order_book and "Ask Price" in merged_order_book:
    # Ordenar por precio (de mayor a menor para bids, menor a mayor para asks)
    merged_order_book = merged_order_book.sort_values(
        by=["Bid Price", "Ask Price"], ascending=[False, True], na_position="last"
    )

    # Unificar order book (asks y bids) en una sola tabla
    # Detectar match
    def detect_match(row):
        return (
            pd.notnull(row["Bid Price"])
            and pd.notnull(row["Ask Price"])
            and row["Bid Price"] >= row["Ask Price"]
        )

    merged_order_book["Match"] = merged_order_book.apply(detect_match, axis=1)

    # Aplicar estilo por fila
    styled_order_book = merged_order_book.style.apply(style_row, axis=1).format(
        {
            "Bid Price": "{:.2f}",
            "Ask Price": "{:.2f}",
            "Bid Quantity": "{:.6f}",
            "Ask Quantity": "{:.6f}",
        }
    )
    st.session_state.styled_order_book = styled_order_book


# Show table
st.title("📘 Unified Order Book with Match Highlighting")


st.dataframe(st.session_state.styled_order_book, use_container_width=True)


# st.subheader("Bids (Buy Orders)")
# st.dataframe(st.session_state.table_data_bids)
# st.subheader("Asks (Sell Orders)")
# st.dataframe(st.session_state.table_data_asks)

# st.table(st.session_state.table_data_eth)
# st.table(st.session_state.table_data_btc)
st.subheader("Unknown data")
st.table(st.session_state.table_data_other)
