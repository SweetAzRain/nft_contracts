use near_contract_standards::non_fungible_token::metadata::TokenMetadata;
use near_sdk::{
    env, near, require, AccountId, PanicOnDefault, NearToken, Promise, Gas, ext_contract,
};

// Определяем внешний интерфейс основного NFT контракта
#[ext_contract(nft_contract)]
trait NFTContract {
    fn nft_mint(
        &mut self,
        token_metadata: TokenMetadata,
    ) -> near_contract_standards::non_fungible_token::Token;
    
    fn nft_transfer(
        &mut self,
        receiver_id: AccountId,
        token_id: String,
        approval_id: Option<u64>,
        memo: Option<String>,
    );

    fn nft_transfer_call(
        &mut self,
        receiver_id: AccountId,
        token_id: String,
        approval_id: Option<u64>,
        memo: Option<String>,
        msg: String,
    ) -> near_sdk::PromiseOrValue<bool>;
    
    fn nft_token(
        &self,
        token_id: String,
    ) -> Option<near_contract_standards::non_fungible_token::Token>;
}

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct ProxyContract {
    pub nft_contract_account_id: AccountId, // test_mint.testnet
    pub treasury_id: AccountId,             // user.testnet
    pub nft_mint_price: NearToken,          // ~0.027 NEAR (цена для вызова NFT-контракта)
    pub required_deposit: NearToken,        // 0.2 NEAR (общая сумма, которую должен приложить пользователь)
    pub owner_id: AccountId,                // Владелец контракта
}

#[near]
impl ProxyContract {
    #[init]
    pub fn new(nft_contract_account_id: AccountId, treasury_id: AccountId) -> Self {
        require!(!env::state_exists(), "Already initialized");
        let owner_id = env::predecessor_account_id(); // Владелец - тот, кто вызвал init
        Self {
            nft_contract_account_id,
            treasury_id,
            // Устанавливаем цены по умолчанию
            nft_mint_price: NearToken::from_yoctonear(27_000_000_000_000_000_000_000), // ~0.027 NEAR
            required_deposit: NearToken::from_yoctonear(200_000_000_000_000_000_000_000), // 0.2 NEAR
            owner_id, // Сохраняем владельца
        }
    }

    /// Установить цену для вызова nft_mint на основном контракте (только владелец)
    pub fn set_nft_mint_price(&mut self, price: NearToken) {
        require!(
            env::predecessor_account_id() == self.owner_id,
            "Unauthorized: Only the contract owner can change the NFT mint price"
        );
        self.nft_mint_price = price;
        env::log_str(&format!("NFT mint price updated to {} yoctoNEAR", price.as_yoctonear()));
    }

    /// Установить обязательную сумму депозита (только владелец)
    pub fn set_required_deposit(&mut self, deposit: NearToken) {
        require!(
            env::predecessor_account_id() == self.owner_id,
            "Unauthorized: Only the contract owner can change the required deposit"
        );
        self.required_deposit = deposit;
        env::log_str(&format!("Required deposit updated to {} yoctoNEAR", deposit.as_yoctonear()));
    }

    /// Установить аккаунт NFT-контракта (только владелец)
    pub fn set_nft_contract_account_id(&mut self, new_account_id: AccountId) {
        require!(
            env::predecessor_account_id() == self.owner_id,
            "Unauthorized: Only the contract owner can change the NFT contract account ID"
        );
        let old_id = self.nft_contract_account_id.clone();
        self.nft_contract_account_id = new_account_id.clone();
        env::log_str(&format!("NFT contract account ID updated from {} to {}", old_id, new_account_id));
    }

    /// Установить аккаунт казначейства (только владелец)
    pub fn set_treasury_id(&mut self, new_treasury_id: AccountId) {
        require!(
            env::predecessor_account_id() == self.owner_id,
            "Unauthorized: Only the contract owner can change the treasury ID"
        );
        let old_id = self.treasury_id.clone();
        self.treasury_id = new_treasury_id.clone();
        env::log_str(&format!("Treasury ID updated from {} to {}", old_id, new_treasury_id));
    }

    /// Передать права владельца (только текущий владелец)
    pub fn set_owner(&mut self, new_owner_id: AccountId) {
        require!(
            env::predecessor_account_id() == self.owner_id,
            "Unauthorized: Only the current owner can transfer ownership"
        );
        let old_owner = self.owner_id.clone();
        self.owner_id = new_owner_id.clone();
        env::log_str(&format!("Ownership transferred from {} to {}", old_owner, new_owner_id));
    }

    #[payable]
    pub fn nft_mint_proxy(
        &mut self,
        token_metadata: TokenMetadata,
    ) -> Promise {
        let deposit = env::attached_deposit();
        require!(
            deposit >= self.required_deposit,
            &format!("Must attach at least {} yoctoNEAR ({} NEAR)", self.required_deposit.as_yoctonear(), self.required_deposit.as_near())
        );

        let sender_id = env::predecessor_account_id();
        // Вычисляем сумму для казначейства
        require!(
            self.required_deposit.as_yoctonear() >= self.nft_mint_price.as_yoctonear(),
            "Configuration error: required_deposit cannot be less than nft_mint_price"
        );
        let treasury_amount = NearToken::from_yoctonear(
            self.required_deposit.as_yoctonear() - self.nft_mint_price.as_yoctonear()
        );

        env::log_str(&format!(
            "Proxy: Received {} yoctoNEAR from {}. Will send {} to treasury and {} to NFT contract.",
            deposit.as_yoctonear(),
            sender_id,
            treasury_amount.as_yoctonear(),
            self.nft_mint_price.as_yoctonear()
        ));

        // 1. Начинаем цепочку вызовов: сначала переводим в казначейство
        Promise::new(self.treasury_id.clone())
            .transfer(treasury_amount)
            // 2. Затем вызываем минт на основном контракте
            .then(
                nft_contract::ext(self.nft_contract_account_id.clone())
                    .with_attached_deposit(self.nft_mint_price)
                    .nft_mint(token_metadata)
            )
            // 3. После минта вызываем callback для перевода токена
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(Gas::from_tgas(35)) // Увеличенный газ для сложной цепочки
                    .callback_mint_complete(sender_id) // Передаем оригинального вызывающего
            )
    }

    /// Callback для обработки результата минта и инициации перевода токена
    #[private] // Доступен только самому контракту
    pub fn callback_mint_complete(
        &mut self,
        original_caller: AccountId,
        #[callback_result] mint_result: Result<near_contract_standards::non_fungible_token::Token, near_sdk::PromiseError>
    ) -> Promise {
        if let Ok(token) = mint_result {
            env::log_str(&format!("Proxy: Successfully minted token {} for {}. Initiating transfer to {}.", token.token_id, env::current_account_id(), original_caller));

            // Вызываем nft_transfer на основном контракте, чтобы передать токен оригинальному вызывающему
            nft_contract::ext(self.nft_contract_account_id.clone())
            .with_attached_deposit(NearToken::from_yoctonear(1)) // <-- Теперь это допустимо и необходимо
                .nft_transfer(
                    original_caller.clone(),         // receiver_id
                    token.token_id.clone(),          // token_id
                    None,                            // approval_id
                    Some(format!("Transfer from proxy {} to original minter {}", env::current_account_id(), original_caller)), // memo
                )
                // После перевода вызываем финальный callback для получения обновленного токена
                .then(
                    Self::ext(env::current_account_id())
                        .with_static_gas(Gas::from_tgas(20))
                        .callback_transfer_complete(token.token_id, original_caller) // Передаем ID токена и оригинального вызывающего
                )
        } else {
            // Обработка ошибки минта
            env::panic_str("Proxy: Failed to mint NFT on the main contract");
        }
    }

    /// Callback для обработки результата перевода и возврата финального результата
    #[private]
    pub fn callback_transfer_complete(
        &mut self,
        token_id: String,
        original_caller: AccountId,
        // ИЗМЕНЕНО: Тип результата соответствует возвращаемому типу nft_transfer (который ничего не возвращает - ())
        #[callback_result] transfer_result: Result<(), near_sdk::PromiseError> 
    ) -> Promise {
        // Обработка результата ()
        if transfer_result.is_ok() {
            env::log_str(&format!("Proxy: Successfully transferred token {} to original caller {}.", token_id, original_caller));
        } else {
            env::log_str(&format!("Proxy: Warning - transfer of token {} to {} failed.", token_id, original_caller));
            // Можно раскомментировать следующую строку, если хотите, чтобы контракт паниковал при ошибке перевода:
            // env::panic_str("Proxy: Failed to transfer NFT");
        }
    
        // Получаем обновленную информацию о токене
        nft_contract::ext(self.nft_contract_account_id.clone())
            .nft_token(token_id)
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(Gas::from_tgas(10))
                    .finalize_get_token_complete()
            )
    }

    #[private]
    pub fn finalize_get_token_complete(
        #[callback_result] token_result: Result<Option<near_contract_standards::non_fungible_token::Token>, near_sdk::PromiseError>
    ) -> near_contract_standards::non_fungible_token::Token {
        match token_result {
            Ok(Some(token)) => {
                env::log_str(&format!("Proxy: Returning updated token info for token_id {}", token.token_id));
                token
            },
            Ok(None) => {
                env::log_str("Proxy: Warning - Token not found after transfer.");
                env::panic_str("Proxy: Token disappeared after minting and transfer");
            }
            Err(_) => {
                env::log_str("Proxy: Warning - Failed to fetch updated token info.");
                env::panic_str("Proxy: Failed to fetch token info after transfer");
            }
        }
    }
}