# client.py
import zmq
import os
import dotenv
import json
import pandas as pd

dotenv.load_dotenv()

zmq_port = os.getenv('ZMQ_PORT', 5555)
print("Using port {}".format(zmq_port))
context = zmq.Context()
socket = context.socket(zmq.REP)
socket.bind(f"tcp://0.0.0.0:{zmq_port}")  # assumes a server is running


def calculate_mid_price(raw_data) -> float:
    data = json.loads(raw_data)  # raw_json_string debe ser tu string completo

    # Extraer los bids y asks
    bids = data["BinanceDepthUpdate"]["b"]
    asks = data["BinanceDepthUpdate"]["a"]

    # Convertir a DataFrames
    df_bids = pd.DataFrame(bids, columns=["Price", "Quantity"]).astype(float)
    df_asks = pd.DataFrame(asks, columns=["Price", "Quantity"]).astype(float)

    # Ordenar como en un libro de órdenes
    df_bids = df_bids.sort_values(by="Price", ascending=False)
    df_asks = df_asks.sort_values(by="Price", ascending=True)
    return (df_bids.iloc[0]["Price"] + df_asks.iloc[0]["Price"]) / 2


while True:
    print("Waiting for message...")
    msg = socket.recv_string()
    print("Received:", msg)

    mid_price = calculate_mid_price(msg)
    resp = {
        "MidPrice": mid_price
    }

    json_str = json.dumps(resp)
    print(json_str)
    socket.send_string(json_str)
    print("Sent reply.")
