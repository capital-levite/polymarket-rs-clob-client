use std::sync::Arc;
use std::str::FromStr;
use polymarket_client_sdk::clob::{Client, Config};
use polymarket_client_sdk::auth::state::Authenticated;
use polymarket_client_sdk::auth::Normal;
use polymarket_client_sdk::clob::types::{OrderType, Side, SignatureType};
use polymarket_client_sdk::clob::types::request::{
    OrderBookSummaryRequest, CancelMarketOrderRequest, OrdersRequest
};
use polymarket_client_sdk::types::{Decimal, Address};
use alloy::signers::local::LocalSigner;
use alloy::signers::Signer;
use pyo3::prelude::*;
use k256::ecdsa::SigningKey;

#[pyclass]
struct PyClient {
    client: Arc<Client<Authenticated<Normal>>>,
    signer: Arc<LocalSigner<SigningKey>>,
}

#[pymethods]
impl PyClient {
    #[staticmethod]
    fn connect(
        py: Python<'_>, 
        host: String, 
        key: String, 
        chain_id: u64, 
        funder: Option<String>, 
        signature_type: Option<u8>
    ) -> PyResult<&PyAny> {
        let key = key.clone();
        pyo3_asyncio::tokio::future_into_py(py, async move {
            let signer = LocalSigner::from_str(&key).map_err(|e| PyErr::new::<pyo3::exceptions::PyValueError, _>(e.to_string()))?;
            let signer = signer.with_chain_id(Some(chain_id));
            
            let client = Client::new(&host, Config::default())
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;
                
            let mut auth_builder = client.authentication_builder(&signer);
            
            if let Some(f) = funder {
                let funder_addr = Address::from_str(&f)
                    .map_err(|e| PyErr::new::<pyo3::exceptions::PyValueError, _>(e.to_string()))?;
                auth_builder = auth_builder.funder(funder_addr);
            }
            
            if let Some(st) = signature_type {
                let st_enum = match st {
                    0 => SignatureType::Eoa,
                    1 => SignatureType::Proxy,
                    2 => SignatureType::GnosisSafe,
                    _ => return Err(PyErr::new::<pyo3::exceptions::PyValueError, _>("Invalid signature type")),
                };
                auth_builder = auth_builder.signature_type(st_enum);
            }
                
            let auth_client = auth_builder.authenticate()
                .await
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;
                
            Ok(PyClient {
                client: Arc::new(auth_client),
                signer: Arc::new(signer),
            })
        })
    }

    fn get_order_book<'a>(&self, py: Python<'a>, token_id: String) -> PyResult<&'a PyAny> {
        let client = self.client.clone();
        pyo3_asyncio::tokio::future_into_py(py, async move {
            let req = OrderBookSummaryRequest::builder()
                .token_id(token_id)
                .build();
            let resp = client.order_book(&req).await
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;
            
            let json = serde_json::to_string(&resp).unwrap();
            Ok(json)
        })
    }
    
    fn get_orders<'a>(&self, py: Python<'a>, marker: Option<String>) -> PyResult<&'a PyAny> {
         let client = self.client.clone();
         pyo3_asyncio::tokio::future_into_py(py, async move {
             // Basic fetch of open orders
             let req = OrdersRequest::default();
             let resp = client.orders(&req, marker).await
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;
             let json = serde_json::to_string(&resp).unwrap();
             Ok(json)
         })
    }

    fn cancel_all<'a>(&self, py: Python<'a>) -> PyResult<&'a PyAny> {
        let client = self.client.clone();
        pyo3_asyncio::tokio::future_into_py(py, async move {
            let resp = client.cancel_all_orders().await
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;
            let json = serde_json::to_string(&resp).unwrap();
            Ok(json)
        })
    }

    fn cancel_order<'a>(&self, py: Python<'a>, order_id: String) -> PyResult<&'a PyAny> {
        let client = self.client.clone();
        pyo3_asyncio::tokio::future_into_py(py, async move {
            let resp = client.cancel_order(&order_id).await
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;
            let json = serde_json::to_string(&resp).unwrap();
            Ok(json)
        })
    }

    fn place_limit_order<'a>(
        &self, 
        py: Python<'a>, 
        token_id: String, 
        side: String, 
        price: f64, 
        size: f64
    ) -> PyResult<&'a PyAny> {
        let client = self.client.clone();
        let signer = self.signer.clone();
        
        pyo3_asyncio::tokio::future_into_py(py, async move {
            let side_enum = match side.to_lowercase().as_str() {
                "buy" => Side::Buy,
                "sell" => Side::Sell,
                _ => return Err(PyErr::new::<pyo3::exceptions::PyValueError, _>("Invalid side")),
            };
            
            let mut price_dec = Decimal::from_f64_retain(price)
                .ok_or_else(|| PyErr::new::<pyo3::exceptions::PyValueError, _>("Invalid price"))?;
            let mut size_dec = Decimal::from_f64_retain(size)
                .ok_or_else(|| PyErr::new::<pyo3::exceptions::PyValueError, _>("Invalid size"))?;

            // Round to avoid floating point noise from Python f64
            price_dec = price_dec.round_dp(6).normalize();
            size_dec = size_dec.round_dp(6).normalize();

            let builder = client.limit_order()
                .token_id(token_id)
                .side(side_enum)
                .price(price_dec)
                .size(size_dec)
                .order_type(OrderType::GTC);

            let signable = builder.build().await
                 .map_err(|e| PyErr::new::<pyo3::exceptions::PyValueError, _>(e.to_string()))?;
            
            let signed = client.sign(&*signer, signable).await
                 .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;
                 
            let resp = client.post_order(signed).await
                 .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;
                 
            let json = serde_json::to_string(&resp).unwrap();
            Ok(json)
        })
    }
    
    // Additional helpers
    fn get_balance_allowance<'a>(&self, py: Python<'a>, asset_type: String, token_id: Option<String>) -> PyResult<&'a PyAny> {
        let client = self.client.clone();
        pyo3_asyncio::tokio::future_into_py(py, async move {
             let asset_type_enum = match asset_type.as_str() {
                "collateral" => polymarket_client_sdk::clob::types::AssetType::Collateral,
                "conditional" => polymarket_client_sdk::clob::types::AssetType::Conditional,
                _ => return Err(PyErr::new::<pyo3::exceptions::PyValueError, _>("Invalid asset type")),
            };
            
            let req = polymarket_client_sdk::clob::types::request::BalanceAllowanceRequest::builder()
                .asset_type(asset_type_enum)
                .maybe_token_id(token_id)
                .build();
            
            let resp = client.balance_allowance(req).await
                 .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;
            
            let json = serde_json::to_string(&resp).unwrap();
            Ok(json)
        })
    }

    fn update_balance_allowance<'a>(&self, py: Python<'a>, asset_type: String, token_id: Option<String>) -> PyResult<&'a PyAny> {
        let client = self.client.clone();
        pyo3_asyncio::tokio::future_into_py(py, async move {
             let asset_type_enum = match asset_type.as_str() {
                "collateral" => polymarket_client_sdk::clob::types::AssetType::Collateral,
                "conditional" => polymarket_client_sdk::clob::types::AssetType::Conditional,
                _ => return Err(PyErr::new::<pyo3::exceptions::PyValueError, _>("Invalid asset type")),
            };
            
            let req = polymarket_client_sdk::clob::types::request::BalanceAllowanceRequest::builder()
                .asset_type(asset_type_enum)
                .maybe_token_id(token_id)
                .build();
            
            client.update_balance_allowance(req).await
                 .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;
            
            Ok(())
        })
    }

    fn get_address<'a>(&self, py: Python<'a>) -> PyResult<&'a PyAny> {
        let address = self.client.address();
        let address_str = address.to_string();
        pyo3_asyncio::tokio::future_into_py(py, async move {
            Ok(address_str)
        })
    }
}

#[pymodule]
fn polymarket_clob_client(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_class::<PyClient>()?;
    Ok(())
}